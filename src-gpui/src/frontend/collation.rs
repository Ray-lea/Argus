//! Name order of the original interface, `Intl.Collator(locale, { numeric: true,
//! sensitivity: "base" })`: ICU collation in the interface language, digit runs
//! compared by value, case and accents ignored. Chinese puts Han first, in pinyin
//! order, then Latin; English puts Latin first. Windows ships ICU as `icu.dll`
//! (Windows 10 1903 and later), loaded here on first use. The original's
//! WebView2 collated with ICU too, and both order the test names alike.
use argus_core::i18n::Locale;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};

/// A string's collation key: keys order as their strings collate.
#[derive(Clone, Default, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SortKey(Arc<[u8]>);

/// Keys of recent names by locale. Process, service and column names repeat on
/// every refresh, so most lookups skip ICU.
struct Keys {
    locale: Option<Locale>,
    collator: Option<icu::Collator>,
    keys: HashMap<Box<str>, SortKey>,
    utf16: Vec<u16>,
    out: Vec<u8>,
}
static KEYS: LazyLock<Mutex<Keys>> = LazyLock::new(|| {
    Mutex::new(Keys {
        locale: None,
        collator: None,
        keys: HashMap::new(),
        utf16: Vec::new(),
        out: Vec::new(),
    })
});
/// Cached keys before the cache starts over; far above the names of one machine.
const CACHED: usize = 16_384;

impl Keys {
    fn key(&mut self, locale: Locale, text: &str) -> SortKey {
        if self.locale != Some(locale) {
            self.locale = Some(locale);
            self.collator = icu::Collator::new(locale.tag());
            self.keys.clear();
        }
        if let Some(key) = self.keys.get(text) {
            return key.clone();
        }
        if self.keys.len() >= CACHED {
            self.keys.clear();
        }
        let key = SortKey(match &self.collator {
            Some(collator) => collator.key(text, &mut self.utf16, &mut self.out),
            // Without ICU, case-insensitive code point order.
            None => text.to_lowercase().into_bytes().into(),
        });
        self.keys.insert(text.into(), key.clone());
        key
    }
}

fn keys() -> std::sync::MutexGuard<'static, Keys> {
    KEYS.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The key of `text` in the interface language.
pub fn key(text: &str) -> SortKey {
    keys().key(argus_core::i18n::locale(), text)
}

/// `a` against `b` as the interface language orders names.
pub fn compare(a: &str, b: &str) -> Ordering {
    let locale = argus_core::i18n::locale();
    let mut keys = keys();
    let a = keys.key(locale, a);
    a.cmp(&keys.key(locale, b))
}

#[cfg(windows)]
mod icu {
    use std::ffi::CString;
    use std::sync::{Arc, OnceLock};
    use windows::Win32::Globalization::{
        UCOL_NORMALIZATION_MODE, UCOL_NUMERIC_COLLATION, UCOL_ON, UCOL_PRIMARY, UCOL_STRENGTH,
        UColAttribute, UColAttributeValue, UCollator, UErrorCode,
    };
    use windows::Win32::System::LibraryLoader::{
        GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
    };
    use windows::core::{PCSTR, s, w};

    type Open = unsafe extern "C" fn(PCSTR, *mut UErrorCode) -> *mut UCollator;
    type SetAttribute =
        unsafe extern "C" fn(*mut UCollator, UColAttribute, UColAttributeValue, *mut UErrorCode);
    type GetSortKey = unsafe extern "C" fn(*const UCollator, *const u16, i32, *mut u8, i32) -> i32;
    type Close = unsafe extern "C" fn(*mut UCollator);
    struct Api {
        open: Open,
        set_attribute: SetAttribute,
        sort_key: GetSortKey,
        close: Close,
    }
    /// The ICU entry points; the module stays loaded for the process's life.
    fn api() -> Option<&'static Api> {
        static API: OnceLock<Option<Api>> = OnceLock::new();
        API.get_or_init(|| unsafe {
            let module = LoadLibraryExW(w!("icu.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32).ok()?;
            Some(Api {
                open: std::mem::transmute::<unsafe extern "system" fn() -> isize, Open>(
                    GetProcAddress(module, s!("ucol_open"))?,
                ),
                set_attribute: std::mem::transmute::<
                    unsafe extern "system" fn() -> isize,
                    SetAttribute,
                >(GetProcAddress(module, s!("ucol_setAttribute"))?),
                sort_key: std::mem::transmute::<unsafe extern "system" fn() -> isize, GetSortKey>(
                    GetProcAddress(module, s!("ucol_getSortKey"))?,
                ),
                close: std::mem::transmute::<unsafe extern "system" fn() -> isize, Close>(
                    GetProcAddress(module, s!("ucol_close"))?,
                ),
            })
        })
        .as_ref()
    }

    /// An ICU collator set up as the original's: primary strength (`sensitivity:
    /// "base"`), numeric ordering, and canonical equivalence as ECMA-402 requires.
    pub struct Collator(*mut UCollator);
    // Used only under the cache's lock.
    unsafe impl Send for Collator {}
    impl Collator {
        pub fn new(tag: &str) -> Option<Self> {
            let api = api()?;
            let tag = CString::new(tag).ok()?;
            let mut status = UErrorCode(0);
            let raw = unsafe { (api.open)(PCSTR(tag.as_ptr().cast()), &mut status) };
            // Warnings (negative codes) report a fallback locale, which is expected.
            if raw.is_null() {
                return None;
            }
            let collator = Self(raw);
            for (attribute, value) in [
                (UCOL_STRENGTH, UCOL_PRIMARY),
                (UCOL_NUMERIC_COLLATION, UCOL_ON),
                (UCOL_NORMALIZATION_MODE, UCOL_ON),
            ] {
                unsafe { (api.set_attribute)(raw, attribute, value, &mut status) };
            }
            (status.0 <= 0).then_some(collator)
        }
        pub fn key(&self, text: &str, utf16: &mut Vec<u16>, out: &mut Vec<u8>) -> Arc<[u8]> {
            let Some(api) = api() else {
                return Arc::default();
            };
            utf16.clear();
            utf16.extend(text.encode_utf16());
            let length = i32::try_from(utf16.len()).unwrap_or(i32::MAX);
            if out.len() < 64 {
                out.resize(64, 0);
            }
            loop {
                let capacity = i32::try_from(out.len()).unwrap_or(i32::MAX);
                let needed = unsafe {
                    (api.sort_key)(self.0, utf16.as_ptr(), length, out.as_mut_ptr(), capacity)
                };
                let Ok(needed) = usize::try_from(needed) else {
                    return Arc::default();
                };
                if needed <= out.len() {
                    // The key ends with a zero byte, which orders nothing.
                    return out[..needed.saturating_sub(1)].into();
                }
                out.resize(needed, 0);
            }
        }
    }
    impl Drop for Collator {
        fn drop(&mut self) {
            if let Some(api) = api() {
                unsafe { (api.close)(self.0) };
            }
        }
    }
}

#[cfg(not(windows))]
mod icu {
    use std::sync::Arc;
    pub struct Collator;
    impl Collator {
        pub fn new(_: &str) -> Option<Self> {
            None
        }
        pub fn key(&self, _: &str, _: &mut Vec<u16>, _: &mut Vec<u8>) -> Arc<[u8]> {
            Arc::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sorts `names` in `locale` without touching the process-wide locale.
    fn sorted(locale: Locale, names: &[&str]) -> Vec<String> {
        let mut keys = keys();
        let mut names: Vec<_> = names
            .iter()
            .map(|name| (keys.key(locale, name), name.to_string()))
            .collect();
        names.sort_by(|a, b| a.0.cmp(&b.0));
        names.into_iter().map(|(_, name)| name).collect()
    }

    // Expected orders are those of the original's WebView2 Intl.Collator.
    const NAMES: [&str; 14] = [
        "Argus",
        "东方财富终端",
        "item 10",
        "ACCUserPS",
        "微信输入法",
        "item 9",
        "终端",
        "网易UU远程",
        "Windows 小组件",
        "中文123",
        "chrome.exe",
        "中文45",
        "10x",
        "9x",
    ];

    #[test]
    fn chinese_puts_han_first_in_pinyin_order_then_latin() {
        assert_eq!(
            sorted(Locale::ZhCn, &NAMES),
            [
                "9x",
                "10x",
                "东方财富终端",
                "网易UU远程",
                "微信输入法",
                "中文45",
                "中文123",
                "终端",
                "ACCUserPS",
                "Argus",
                "chrome.exe",
                "item 9",
                "item 10",
                "Windows 小组件",
            ]
        );
    }

    #[test]
    fn english_puts_latin_first_and_han_by_code_point() {
        assert_eq!(
            sorted(Locale::EnUs, &NAMES),
            [
                "9x",
                "10x",
                "ACCUserPS",
                "Argus",
                "chrome.exe",
                "item 9",
                "item 10",
                "Windows 小组件",
                "东方财富终端",
                "中文45",
                "中文123",
                "微信输入法",
                "终端",
                "网易UU远程",
            ]
        );
    }

    #[test]
    fn case_and_accents_do_not_order_names() {
        let mut keys = keys();
        for locale in Locale::ALL {
            for (a, b) in [("Résumé", "RESUME"), ("Zeta", "zeta"), ("ärger", "Arger")] {
                assert_eq!(keys.key(locale, a), keys.key(locale, b), "{a} {b}");
            }
            assert!(keys.key(locale, "item09") < keys.key(locale, "item10"));
            assert!(keys.key(locale, "a2b") < keys.key(locale, "a10b"));
        }
    }
}
