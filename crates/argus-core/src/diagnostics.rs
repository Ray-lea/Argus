//! Public diagnostic logs accept classifications, never free-form errors.
//! Redacting a few path patterns cannot protect arbitrary exception payloads.

/// Classify an error without retaining any text supplied by an OS, driver,
/// remote endpoint, application or user. The original error can still be shown
/// locally in the UI, but is never copied to a diagnostic log.
pub fn failure(text: &str) -> &'static str {
    let text = text
        .chars()
        .take(4096)
        .collect::<String>()
        .to_ascii_lowercase();
    if text.contains("access denied")
        || text.contains("permission denied")
        || text.contains("0x80070005")
    {
        "access denied"
    } else if text.contains("timed out") || text.contains("timeout") || text.contains("0x800705b4")
    {
        "timeout"
    } else if text.contains("cancel") || text.contains("0x800704c7") {
        "cancelled"
    } else if text.contains("signature") || text.contains("certificate") {
        "signature or certificate validation failed"
    } else if text.contains("not found")
        || text.contains("does not exist")
        || text.contains("0x80070002")
    {
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
    fn only_fixed_action_and_exception_names_are_recorded() {
        assert_eq!(action("install"), "install");
        assert_eq!(exception("TypeError"), "TypeError");
        assert_eq!(action(r"C:\Users\Someone\tool"), action("unknown"));
        assert_eq!(exception("TypeError: Alice Smith"), exception("unknown"));
    }
}
