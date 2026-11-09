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
    #[serde(rename = "zh-TW")]
    ZhTw,
    #[serde(rename = "en-US")]
    EnUs,
    #[serde(rename = "es-ES")]
    EsEs,
    #[serde(rename = "de-DE")]
    DeDe,
    #[serde(rename = "ja-JP")]
    JaJp,
    #[serde(rename = "fr-FR")]
    FrFr,
    #[serde(rename = "ru-RU")]
    RuRu,
}

impl Locale {
    /// The catalog every other one is translated from, and the fallback for a
    /// missing key.
    pub const SOURCE: Locale = Locale::ZhCn;
    pub const ALL: [Locale; 8] = [
        Self::ZhCn,
        Self::ZhTw,
        Self::EnUs,
        Self::EsEs,
        Self::DeDe,
        Self::JaJp,
        Self::FrFr,
        Self::RuRu,
    ];

    pub fn tag(self) -> &'static str {
        match self {
            Locale::ZhCn => "zh-CN",
            Locale::ZhTw => "zh-TW",
            Locale::EnUs => "en-US",
            Locale::EsEs => "es-ES",
            Locale::DeDe => "de-DE",
            Locale::JaJp => "ja-JP",
            Locale::FrFr => "fr-FR",
            Locale::RuRu => "ru-RU",
        }
    }

    pub fn from_tag(tag: &str) -> Option<Locale> {
        let mut parts = tag.trim().split(['-', '_']);
        let language = parts.next()?;
        if language.eq_ignore_ascii_case("zh") {
            let subtags: Vec<_> = parts.take_while(|part| part.len() > 1).collect();
            // An explicit script takes precedence over the region.
            for part in &subtags {
                if part.eq_ignore_ascii_case("Hant") || part.eq_ignore_ascii_case("CHT") {
                    return Some(Self::ZhTw);
                }
                if part.eq_ignore_ascii_case("Hans") || part.eq_ignore_ascii_case("CHS") {
                    return Some(Self::ZhCn);
                }
            }
            return Some(
                if subtags.iter().any(|part| {
                    ["TW", "HK", "MO"]
                        .iter()
                        .any(|region| part.eq_ignore_ascii_case(region))
                }) {
                    Self::ZhTw
                } else {
                    Self::ZhCn
                },
            );
        }
        Self::ALL
            .into_iter()
            .find(|locale| locale.tag()[..2].eq_ignore_ascii_case(language))
    }

    /// Regional variants share a catalog; Windows LANGIDs encode the primary
    /// language in their low ten bits (including Spanish's multiple LCIDs).
    pub fn from_langid(id: u16) -> Option<Self> {
        match id & 0x3ff {
            0x04 => Some(match id >> 10 {
                0x01 | 0x03 | 0x05 | 0x1f => Self::ZhTw,
                _ => Self::ZhCn,
            }),
            0x09 => Some(Self::EnUs),
            0x0a => Some(Self::EsEs),
            0x07 => Some(Self::DeDe),
            0x11 => Some(Self::JaJp),
            0x0c => Some(Self::FrFr),
            0x19 => Some(Self::RuRu),
            _ => None,
        }
    }

    /// The display language, then the user's ordered fallback languages.
    /// Regional formatting and the keyboard layout do not select UI text.
    pub fn system() -> Locale {
        use windows::Win32::Globalization::{
            GetUserDefaultUILanguage, GetUserPreferredUILanguages, MUI_LANGUAGE_NAME,
        };
        use windows::core::PWSTR;
        if let Some(locale) = Self::from_langid(unsafe { GetUserDefaultUILanguage() }) {
            return locale;
        }
        let (mut count, mut size) = (0, 0);
        unsafe {
            if GetUserPreferredUILanguages(MUI_LANGUAGE_NAME, &mut count, None, &mut size).is_ok()
                && size > 0
                && size <= 65536
            {
                let mut buffer = vec![0u16; size as usize];
                if GetUserPreferredUILanguages(
                    MUI_LANGUAGE_NAME,
                    &mut count,
                    Some(PWSTR(buffer.as_mut_ptr())),
                    &mut size,
                )
                .is_ok()
                {
                    for tag in buffer.split(|c| *c == 0).filter(|tag| !tag.is_empty()) {
                        if let Some(locale) = Self::from_tag(&String::from_utf16_lossy(tag)) {
                            return locale;
                        }
                    }
                }
            }
        }
        Self::EnUs
    }

    fn catalog(self) -> &'static HashMap<String, String> {
        static CATALOGS: [OnceLock<HashMap<String, String>>; Locale::ALL.len()] =
            [const { OnceLock::new() }; Locale::ALL.len()];
        CATALOGS[self as usize].get_or_init(|| {
            let json = match self {
                Locale::ZhCn => include_str!("../../../locales/zh-CN/native.json"),
                Locale::ZhTw => include_str!("../../../locales/zh-TW/native.json"),
                Locale::EnUs => include_str!("../../../locales/en-US/native.json"),
                Locale::EsEs => include_str!("../../../locales/es-ES/native.json"),
                Locale::DeDe => include_str!("../../../locales/de-DE/native.json"),
                Locale::JaJp => include_str!("../../../locales/ja-JP/native.json"),
                Locale::FrFr => include_str!("../../../locales/fr-FR/native.json"),
                Locale::RuRu => include_str!("../../../locales/ru-RU/native.json"),
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

static APPLICATION_CATALOGS: [OnceLock<HashMap<String, String>>; Locale::ALL.len()] =
    [const { OnceLock::new() }; Locale::ALL.len()];

/// Registers additional application catalogs without coupling collectors or
/// service executables to a particular user interface.
pub fn register_catalog(locale: Locale, sections: &[(&str, &str)]) {
    APPLICATION_CATALOGS[locale as usize].get_or_init(|| {
        let mut map = HashMap::new();
        for (namespace, json) in sections {
            if let Ok(value) = serde_json::from_str(json) {
                flatten(namespace, value, &mut map);
            }
        }
        map
    });
}

/// Set on a child process's command to have it use the parent's locale.
pub const ENV: &str = "ARGUS_LOCALE";

pub fn locale() -> Locale {
    match CURRENT.load(Ordering::Relaxed) {
        UNSET => {
            let initial = std::env::var(ENV)
                .ok()
                .and_then(|tag| Locale::from_tag(&tag))
                .unwrap_or_else(Locale::system);
            match CURRENT.compare_exchange(
                UNSET,
                initial as u8,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => initial,
                Err(n) => Locale::ALL[n as usize],
            }
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
    format_in(locale(), key, args)
}

/// Formats without changing the process-wide language (also used by tests).
pub fn format_in(locale: Locale, key: &str, args: &[(&str, &dyn Display)]) -> String {
    let count = args
        .iter()
        .find(|(name, _)| *name == "count")
        .map(|(_, value)| value.to_string());
    format_args(locale, key, args, count.as_deref())
}

/// Selects a plural from the raw number while `args` may display a regional count.
pub fn format_count(key: &str, count: impl Display, args: &[(&str, &dyn Display)]) -> String {
    format_args(locale(), key, args, Some(&count.to_string()))
}

fn format_args(
    locale: Locale,
    key: &str,
    args: &[(&str, &dyn Display)],
    count: Option<&str>,
) -> String {
    for candidate in [locale, Locale::EnUs, Locale::SOURCE] {
        if let Some(count) = count {
            for form in [plural(candidate, count), "other"] {
                if let Some(template) = entry(candidate, &format!("{key}.{form}")) {
                    return render(template, args);
                }
            }
        }
        if let Some(template) = entry(candidate, key) {
            return render(template, args);
        }
    }
    key.to_owned()
}

/// CLDR cardinal categories for the supported languages. Counts are passed
/// as raw numbers, before display formatting. Visible fractions matter in Russian.
fn plural(locale: Locale, count: &str) -> &'static str {
    let Ok(n) = count.parse::<f64>() else {
        return "other";
    };
    if !n.is_finite() {
        return "other";
    }
    let n = n.abs();
    let integer = count
        .trim_start_matches(['-', '+'])
        .split('.')
        .next()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(n.trunc() as u64);
    let fraction = count
        .split_once('.')
        .is_some_and(|(_, digits)| !digits.is_empty());
    match locale {
        Locale::ZhCn | Locale::ZhTw | Locale::JaJp => "other",
        Locale::RuRu if !fraction && n.fract() == 0. => match (integer % 10, integer % 100) {
            (1, last) if last != 11 => "one",
            (2..=4, last) if !(12..=14).contains(&last) => "few",
            _ => "many",
        },
        Locale::FrFr if integer <= 1 => "one",
        Locale::FrFr | Locale::EsEs
            if n != 0. && !fraction && n.fract() == 0. && integer.is_multiple_of(1_000_000) =>
        {
            "many"
        }
        Locale::EsEs if n == 1. => "one",
        Locale::EnUs | Locale::DeDe if n == 1. && !fraction => "one",
        _ => "other",
    }
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

fn entry(locale: Locale, key: &str) -> Option<&'static str> {
    locale
        .catalog()
        .get(key)
        .or_else(|| {
            APPLICATION_CATALOGS[locale as usize]
                .get()
                .and_then(|map| map.get(key))
        })
        .map(String::as_str)
}

fn lookup(locale: Locale, key: &str) -> &str {
    [locale, Locale::EnUs, Locale::SOURCE]
        .into_iter()
        .find_map(|candidate| {
            entry(candidate, key).or_else(|| entry(candidate, &format!("{key}.other")))
        })
        .unwrap_or(key)
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
        fn keys(catalog: &HashMap<String, String>) -> std::collections::BTreeSet<&str> {
            catalog
                .keys()
                .map(|key| {
                    key.rsplit_once('.')
                        .filter(|(_, form)| matches!(*form, "one" | "few" | "many" | "other"))
                        .map_or(key.as_str(), |(root, _)| root)
                })
                .collect()
        }
        let source = Locale::SOURCE.catalog();
        assert!(!source.is_empty());
        for locale in Locale::ALL {
            let catalog = locale.catalog();
            assert_eq!(keys(catalog), keys(source), "{}", locale.tag());
        }
    }

    #[test]
    fn regions_and_scripts_resolve_without_changing_global_language() {
        for (tag, locale) in [
            ("ES-mx", Locale::EsEs),
            ("de_AT", Locale::DeDe),
            ("fr-CA", Locale::FrFr),
            ("ja", Locale::JaJp),
            ("ru-KZ", Locale::RuRu),
            ("zh-Hant-TW", Locale::ZhTw),
            ("zh_HK", Locale::ZhTw),
            ("zh-MO", Locale::ZhTw),
            ("zh-TW", Locale::ZhTw),
            ("zh-Hant-CN", Locale::ZhTw),
            ("zh-Hans-TW", Locale::ZhCn),
            ("zh-CHT", Locale::ZhTw),
            ("zh-CHS", Locale::ZhCn),
            ("zh-SG", Locale::ZhCn),
            ("zh", Locale::ZhCn),
            ("zh-CN-x-hant", Locale::ZhCn),
            ("en-GB", Locale::EnUs),
        ] {
            assert_eq!(Locale::from_tag(tag), Some(locale));
        }
        for tag in ["", "system", "des", "pt-BR"] {
            assert_eq!(Locale::from_tag(tag), None);
        }
        for (id, locale) in [
            (0x0c0a, Locale::EsEs),
            (0x080a, Locale::EsEs),
            (0x0807, Locale::DeDe),
            (0x0c0c, Locale::FrFr),
            (0x0411, Locale::JaJp),
            (0x0419, Locale::RuRu),
            (0x0404, Locale::ZhTw),
            (0x0c04, Locale::ZhTw),
            (0x1404, Locale::ZhTw),
            (0x7c04, Locale::ZhTw),
            (0x0804, Locale::ZhCn),
            (0x1004, Locale::ZhCn),
            (0x0004, Locale::ZhCn),
        ] {
            assert_eq!(Locale::from_langid(id), Some(locale));
        }
        for locale in Locale::ALL {
            assert_eq!(Locale::from_tag(locale.tag()), Some(locale));
        }
    }

    #[test]
    fn cardinal_rules_cover_zero_teens_decimals_and_millions() {
        for (count, form) in [
            ("0", "many"),
            ("1", "one"),
            ("2", "few"),
            ("5", "many"),
            ("11", "many"),
            ("14", "many"),
            ("21", "one"),
            ("22", "few"),
            ("25", "many"),
            ("101", "one"),
            ("1.0", "other"),
            ("1.5", "other"),
            ("-22", "few"),
        ] {
            assert_eq!(plural(Locale::RuRu, count), form, "{count}");
        }
        for locale in [Locale::ZhCn, Locale::ZhTw, Locale::JaJp] {
            for count in ["0", "1", "2", "1.5"] {
                assert_eq!(plural(locale, count), "other");
            }
        }
        for count in ["0", "1", "1.5"] {
            assert_eq!(plural(Locale::FrFr, count), "one");
        }
        for locale in [Locale::FrFr, Locale::EsEs] {
            assert_eq!(plural(locale, "1000000"), "many");
            assert_eq!(plural(locale, "1000000.0"), "other");
        }
        assert_eq!(plural(Locale::RuRu, "9007199254741021"), "one");
        assert_eq!(plural(Locale::EsEs, "1.0"), "one");
        for locale in [Locale::EnUs, Locale::DeDe] {
            assert_eq!(plural(locale, "1.0"), "other");
        }
    }

    #[test]
    fn renders_placeholders() {
        let args: [(&str, &dyn Display); 2] = [("a", &1), ("b", &"x")];
        assert_eq!(render("{a} and {b}, {a}", &args), "1 and x, 1");
        assert_eq!(render("{c} {a", &args), "{c} {a");
        assert_eq!(lookup(Locale::EnUs, "no.such.key"), "no.such.key");
    }

    #[test]
    fn grouped_counts_keep_the_raw_plural_category() {
        let args: [(&str, &dyn Display); 1] = [("count", &"1.021")];
        let template = entry(Locale::RuRu, "installer.moreProcesses.one").unwrap();
        assert_eq!(
            format_args(Locale::RuRu, "installer.moreProcesses", &args, Some("1021")),
            render(template, &args)
        );
    }
}
