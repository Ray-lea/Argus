//! Public diagnostic logs accept classifications, never free-form errors.
//! Redacting a few path patterns cannot protect arbitrary exception payloads.

use std::borrow::Cow;

/// Classify an error without retaining any text supplied by an OS, driver,
/// remote endpoint, application or user. The original error can still be shown
/// locally in the UI, but is never copied to a diagnostic log. A Windows error
/// code is kept: the message around it is in the system's language, the code
/// is not, and it names no person or file.
pub fn failure(text: &str) -> Cow<'static, str> {
    let text = text
        .chars()
        .take(4096)
        .collect::<String>()
        .to_ascii_lowercase();
    if let Some(code) = os_error(&text) {
        return match category(code) {
            Some(name) => format!("{name} (os error {code})").into(),
            None => format!("os error {code}").into(),
        };
    }
    if let Some(hresult) = hresults(&text).next() {
        return format!("hresult {hresult:#010x}").into();
    }
    keywords(&text).into()
}

/// The Win32 code as Rust ("(os error 5)") and windows-rs ("(0x80070005)")
/// print it.
fn os_error(text: &str) -> Option<u32> {
    let rust = text.match_indices("os error ").find_map(|(at, _)| {
        if joined(text, at) {
            return None;
        }
        let digits = &text[at + 9..];
        digits[..digits.bytes().take_while(u8::is_ascii_digit).count()]
            .parse()
            .ok()
    });
    rust.or_else(|| hresults(text).find_map(|h| (h >> 16 == 0x8007).then_some(h & 0xffff)))
}

/// Failure HRESULTs: exactly eight hex digits with the top bit set, so that
/// addresses and other numbers in the text are not taken for one.
fn hresults(text: &str) -> impl Iterator<Item = u32> + '_ {
    text.match_indices("0x").filter_map(|(at, _)| {
        let digits = &text[at + 2..];
        if joined(text, at) || digits.bytes().take_while(u8::is_ascii_hexdigit).count() != 8 {
            return None;
        }
        u32::from_str_radix(&digits[..8], 16)
            .ok()
            .filter(|h| h >> 31 == 1)
    })
}

/// Whether the match at `at` continues a longer word or number.
fn joined(text: &str, at: usize) -> bool {
    text[..at]
        .bytes()
        .next_back()
        .is_some_and(|b| b.is_ascii_alphanumeric())
}

/// Names for the codes Argus usually meets; the rest are logged by number.
fn category(code: u32) -> Option<&'static str> {
    Some(match code {
        2 | 3 | 1060 => "not found",
        5 => "access denied",
        32 | 33 | 170 => "in use",
        80 | 183 => "already exists",
        13 | 87 => "invalid or unsupported data",
        258 | 1460 | 10060 | 12002 => "timeout",
        1223 => "cancelled",
        10061 | 11001 | 12007 | 12029 => "network error",
        _ => return None,
    })
}

fn keywords(text: &str) -> &'static str {
    if text.contains("access denied")
        || text.contains("access is denied")
        || text.contains("permission denied")
    {
        "access denied"
    } else if text.contains("timed out") || text.contains("timeout") {
        "timeout"
    } else if text.contains("cancel") {
        "cancelled"
    } else if text.contains("signature") || text.contains("certificate") {
        "signature or certificate validation failed"
    } else if text.contains("not found") || text.contains("does not exist") {
        "not found"
    } else if text.contains("network")
        || text.contains("connection")
        || text.contains("http")
        || text.contains("dns")
    {
        "network error"
    } else if text.contains("invalid")
        || text.contains("unsupported")
        || text.contains("incompatible")
    {
        "invalid or unsupported data"
    } else {
        "operation failed"
    }
}

pub fn action(value: &str) -> &'static str {
    match value {
        "install" => "install",
        "uninstall" => "uninstall",
        "update" => "update",
        "start" => "start",
        "enable" => "enable",
        "restore" => "restore",
        _ => "unknown action",
    }
}

pub fn exception(value: &str) -> &'static str {
    match value {
        "TypeError" => "TypeError",
        "RangeError" => "RangeError",
        "ReferenceError" => "ReferenceError",
        "SyntaxError" => "SyntaxError",
        "URIError" => "URIError",
        "EvalError" => "EvalError",
        "AbortError" => "AbortError",
        "NetworkError" => "NetworkError",
        "Error" => "Error",
        _ => "unknown exception",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arbitrary_error_payloads_never_reach_the_log() {
        for text in [
            r"cannot open C:\Users\Alice Smith\Documents\private.txt: access denied",
            r"cannot open D:\Work\private\project.docx: not found",
            r"failed \\server\share\秘密\document.txt",
            r"failed \\?\C:\Program Files\private tool.exe --token top-secret",
            "failed file:///C:/Users/Alice%20Smith/private.txt",
            "failed https://alice:password@example.invalid/path?token=top-secret",
            "user Alice Smith at 192.0.2.10 (alice@example.invalid)",
            "TypeError: --password top-secret\n    at C:\\private\\app.js:10:20",
            "坏了：用户甲的私人文件", // i18n-ignore: synthetic private data, never a UI message.
        ] {
            let output = failure(text);
            assert!(!output.contains(['\\', '/', '@', ':', '\n']));
            for private in [
                "Alice",
                "Smith",
                "private",
                "top-secret",
                "password",
                "192.0.2.10",
                "example.invalid",
            ] {
                assert!(!output.contains(private), "{output}");
            }
        }
    }

    #[test]
    fn windows_error_codes_are_classified_in_any_language() {
        for (text, class) in [
            (
                "启动自检失败：拒绝访问。 (os error 5)（5）",
                "access denied (os error 5)",
            ),
            (
                "The startup self-test failed: Access is denied. (os error 5) (5)",
                "access denied (os error 5)",
            ),
            (
                "Zugriff verweigert (0x80070005)",
                "access denied (os error 5)",
            ),
            (
                "系统找不到指定的文件。 (os error 2)",
                "not found (os error 2)",
            ),
            (
                "另一个程序正在使用此文件，进程无法访问。 (os error 32)",
                "in use (os error 32)",
            ),
            (
                "操作已被用户取消。 (0x800704C7)",
                "cancelled (os error 1223)",
            ),
            ("信号灯超时时间已到。 (os error 121)", "os error 121"),
            ("没有注册类 (0x80040154)", "hresult 0x80040154"),
        ] {
            assert_eq!(failure(text), class, "{text}");
        }
    }

    #[test]
    fn other_numbers_in_the_text_are_not_taken_for_codes() {
        for (text, class) in [
            (
                r"C:\Users\user1001\cache 0x7ff6a2b31000\data.bin: access denied",
                "access denied",
            ),
            ("build 26300.9711 failed with 0x1234", "operation failed"),
            ("id ab0x80070005", "operation failed"),
            ("cmos error 5", "operation failed"),
        ] {
            assert_eq!(failure(text), class, "{text}");
        }
    }

    #[test]
    fn a_classification_classifies_as_itself() {
        // A classification can be checked again wherever it is logged.
        for text in [
            "拒绝访问。 (os error 5)",
            "(os error 121)",
            "Class not registered (0x80040154)",
            "the operation timed out",
            "something else",
        ] {
            let once = failure(text);
            assert_eq!(failure(&once), once, "{text}");
        }
    }

    #[test]
    fn only_fixed_action_and_exception_names_are_recorded() {
        assert_eq!(action("install"), "install");
        assert_eq!(exception("TypeError"), "TypeError");
        assert_eq!(action(r"C:\Users\Someone\tool"), action("unknown"));
        assert_eq!(exception("TypeError: Alice Smith"), exception("unknown"));
    }
}
