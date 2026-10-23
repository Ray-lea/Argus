//! Release metadata shared by the application adapters.
pub const PROXY_PRESETS: [&str; 3] = [
    "https://gh-proxy.com/",
    "https://gh-proxy.org/",
    "https://ghfast.top/",
];
static MANIFEST: std::sync::OnceLock<serde_json::Value> = std::sync::OnceLock::new();

pub fn product_name() -> &'static str {
    manifest()["productName"].as_str().expect("product name")
}

pub fn identifier() -> &'static str {
    manifest()["identifier"]
        .as_str()
        .expect("application identifier")
}
pub fn valid_identifier(identifier: &str) -> bool {
    identifier.len() <= 128
        && identifier.contains('.')
        && identifier.split('.').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
        })
}
pub fn manifest() -> &'static serde_json::Value {
    MANIFEST.get_or_init(|| {
        serde_json::from_str(include_str!("../../../resources/app.json"))
            .expect("application manifest")
    })
}
pub fn app_title() -> String {
    if cfg!(debug_assertions) {
        argus_core::tr!("app.devTitle")
    } else {
        product_name().into()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn identifiers_cannot_escape_the_application_directory() {
        for value in [
            "",
            ".",
            "..",
            "...",
            "dev/argus",
            "dev\\argus",
            "dev.argus.",
            ".dev.argus",
        ] {
            assert!(!super::valid_identifier(value));
        }
        assert!(super::valid_identifier(super::identifier()));
    }
}
