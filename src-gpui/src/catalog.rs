//! The desktop registers its existing catalogs with the shared translation API.
pub fn register() {
    use argus_core::i18n::{Locale, register_catalog};
    macro_rules! catalogs { ($locale:literal,$($name:literal),*) => { &[$(($name,include_str!(concat!("../../locales/",$locale,"/",$name,".json")))),*] }; }
    macro_rules! register {
        ($locale:ident, $tag:literal) => {
            register_catalog(
                Locale::$locale,
                catalogs!(
                    $tag,
                    "actions",
                    "common",
                    "connections",
                    "details",
                    "fileLock",
                    "format",
                    "history",
                    "onboarding",
                    "performance",
                    "processes",
                    "services",
                    "settings",
                    "shell",
                    "startup",
                    "tasks",
                    "updates"
                ),
            );
        };
    }
    register!(ZhCn, "zh-CN");
    register!(ZhTw, "zh-TW");
    register!(EnUs, "en-US");
    register!(EsEs, "es-ES");
    register!(DeDe, "de-DE");
    register!(JaJp, "ja-JP");
    register!(FrFr, "fr-FR");
    register!(RuRu, "ru-RU");
}
