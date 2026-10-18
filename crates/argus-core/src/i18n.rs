//! User-facing text of the native layers (tray, errors, service messages),
//! looked up by key in `locales/<locale>/native.json`, the same catalogs the
//! UI's are kept beside. See `docs/development/i18n.md`.
//!
//! The locale is process-wide. It starts as the one a parent process passed in
//! [`ENV`] (helpers the GUI starts), otherwise the Windows display language;
//! the GUI host then applies the user's choice. The privileged service and the
//! elevated helpers never see that choice and keep the display language.

use std::collections::HashMap;
use std::fmt::Display;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU8, Ordering};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Locale {
    #[serde(rename = "zh-CN")]
    ZhCn,
    #[serde(rename = "en-US")]
    EnUs,
}

impl Locale {
    /// The catalog every other one is translated from, and the fallback for a
    /// missing key.
    pub const SOURCE: Locale = Locale::ZhCn;
    pub const ALL: [Locale; 2] = [Locale::ZhCn, Locale::EnUs];

    pub fn tag(self) -> &'static str {
        match self {
            Locale::ZhCn => "zh-CN",
            Locale::EnUs => "en-US",
        }
    }

    pub fn from_tag(tag: &str) -> Option<Locale> {
        Locale::ALL
            .into_iter()
            .find(|l| l.tag().eq_ignore_ascii_case(tag))
    }

    /// The Windows display language: Chinese in any script reads the
    /// Simplified Chinese text, everything else English.
    pub fn system() -> Locale {
        const LANG_CHINESE: u16 = 0x04;
        let lang = unsafe { windows::Win32::Globalization::GetUserDefaultUILanguage() };
        if lang & 0x3ff == LANG_CHINESE {
            Locale::ZhCn
        } else {
            Locale::EnUs
        }
    }

    fn catalog(self) -> &'static HashMap<String, String> {
        static CATALOGS: [OnceLock<HashMap<String, String>>; 2] =
            [OnceLock::new(), OnceLock::new()];
        CATALOGS[self as usize].get_or_init(|| {
            let json = match self {
                Locale::ZhCn => include_str!("../../../locales/zh-CN/native.json"),
                Locale::EnUs => include_str!("../../../locales/en-US/native.json"),
            };
            let mut map = HashMap::new();
            // The catalogs are checked by scripts/i18n.mjs; a broken one only
            // leaves keys showing.
            if let Ok(value) = serde_json::from_str(json) {
                flatten("", value, &mut map);
            }
            map
        })
    }
}

/// Nested objects become dotted keys: `{"tray": {"quit": "…"}}` is `tray.quit`.
fn flatten(prefix: &str, value: serde_json::Value, out: &mut HashMap<String, String>) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, value) in map {
                let key = if prefix.is_empty() {
                    key
                } else {
                    format!("{prefix}.{key}")
                };
                flatten(&key, value, out);
            }
        }
        serde_json::Value::String(text) => {
            out.insert(prefix.to_string(), text);
        }
        _ => {}
    }
}

const UNSET: u8 = u8::MAX;
static CURRENT: AtomicU8 = AtomicU8::new(UNSET);

/// Set on a child process's command to have it use the parent's locale.
pub const ENV: &str = "ARGUS_LOCALE";

pub fn locale() -> Locale {
    match CURRENT.load(Ordering::Relaxed) {
        UNSET => {
            let initial = std::env::var(ENV)
                .ok()
                .and_then(|tag| Locale::from_tag(&tag))
                .unwrap_or_else(Locale::system);
            CURRENT.store(initial as u8, Ordering::Relaxed);
            initial
        }
        n => Locale::ALL[n as usize],
    }
}

pub fn set_locale(locale: Locale) {
    CURRENT.store(locale as u8, Ordering::Relaxed);
}

/// The text for `key` in the current locale; use [`tr!`](crate::tr).
pub fn text(key: &str) -> String {
    lookup(locale(), key).to_string()
}

/// The text for `key` in a given locale, for text that must not depend on the
/// current one (a file several people may read).
pub fn text_in(locale: Locale, key: &str) -> String {
    lookup(locale, key).to_string()
}

/// The text for `key` with each `{name}` replaced; use [`tr!`](crate::tr).
pub fn format(key: &str, args: &[(&str, &dyn Display)]) -> String {
    render(lookup(locale(), key), args)
}

fn render(template: &str, args: &[(&str, &dyn Display)]) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let tail = &rest[start + 1..];
        let value = tail
            .find('}')
            .and_then(|end| Some((args.iter().find(|(n, _)| *n == &tail[..end])?, end)));
        match value {
            Some(((_, value), end)) => {
                out.push_str(&value.to_string());
                rest = &tail[end + 1..];
            }
            None => {
                out.push('{');
                rest = tail;
            }
        }
    }
    out.push_str(rest);
    out
}

fn lookup(locale: Locale, key: &str) -> &str {
    locale
        .catalog()
        .get(key)
        .or_else(|| Locale::SOURCE.catalog().get(key))
        .map_or(key, String::as_str)
}

/// Looks up user-facing text: `tr!("tray.quit")`, or with placeholders,
/// `tr!("tray.update", version = v)` for `"更新到 {version}…"`.
/// scripts/i18n.mjs checks that every key used this way exists.
#[macro_export]
macro_rules! tr {
    ($key:literal) => {
        $crate::i18n::text($key)
    };
    ($key:literal, $($name:ident = $value:expr),+ $(,)?) => {
        $crate::i18n::format(
            $key,
            &[$((stringify!($name), &$value as &dyn ::std::fmt::Display)),+],
        )
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalogs_parse_and_share_keys() {
        let source = Locale::SOURCE.catalog();
        assert!(!source.is_empty());
        for locale in Locale::ALL {
            let catalog = locale.catalog();
            assert_eq!(catalog.len(), source.len(), "{}", locale.tag());
            assert!(
                source.keys().all(|k| catalog.contains_key(k)),
                "{}",
                locale.tag()
            );
        }
    }

    #[test]
    fn renders_placeholders() {
        let args: [(&str, &dyn Display); 2] = [("a", &1), ("b", &"x")];
        assert_eq!(render("{a} and {b}, {a}", &args), "1 and x, 1");
        assert_eq!(render("{c} {a", &args), "{c} {a");
        assert_eq!(lookup(Locale::EnUs, "no.such.key"), "no.such.key");
    }
}
