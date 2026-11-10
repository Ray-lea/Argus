//! Release metadata shared by the application adapters.
pub const PROXY_PRESETS: [&str; 3] = [
    "https://gh-proxy.com/",
    "https://gh-proxy.org/",
    "https://ghfast.top/",
];
static MANIFEST: std::sync::OnceLock<serde_json::Value> = std::sync::OnceLock::new();

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum BuildKind {
    Development,
    Test,
    Stable,
}

impl BuildKind {
    pub fn label_key(self) -> &'static str {
        match self {
            Self::Development => "common.build.development",
            Self::Test => "common.build.test",
            Self::Stable => "common.build.stable",
        }
    }
}

/// The installed artifact's identity is independent of its update feed.
pub fn build_kind(display_version: &str, development: bool) -> BuildKind {
    if development {
        BuildKind::Development
    } else if display_version.contains(['-', '+']) {
        BuildKind::Test
    } else {
        BuildKind::Stable
    }
}

pub fn product_name() -> &'static str {
    manifest()["productName"].as_str().expect("product name")
}

pub fn identifier() -> &'static str {
    manifest()["identifier"]
        .as_str()
        .expect("application identifier")
}
/// A compiled development profile remains stable when the executable is copied
/// for hot reload, while independent worktrees never share settings or locks.
pub fn development_identifier(workspace: &str) -> String {
    use sha2::{Digest, Sha256};
    let workspace = if cfg!(windows) {
        workspace.replace('\\', "/").to_lowercase()
    } else {
        workspace.into()
    };
    let suffix = Sha256::digest(workspace.as_bytes())[..6]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("{}.devbuild.{suffix}", identifier())
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
    fn build_identity_distinguishes_profiles_and_non_release_versions() {
        use super::{BuildKind, build_kind};
        assert_eq!(build_kind("0.4.0", false), BuildKind::Stable);
        for version in [
            "0.4.0+672.g1234567",
            "0.4.1-beta.2",
            "0.4.1-rc.1+673.g7654321",
        ] {
            assert_eq!(build_kind(version, false), BuildKind::Test);
            assert_eq!(build_kind(version, true), BuildKind::Development);
        }
        assert_eq!(build_kind("0.4.0", true), BuildKind::Development);
    }

    #[test]
    fn development_profiles_are_stable_and_isolated_by_workspace() {
        let one = super::development_identifier("D:/Projects/Argus/src-gpui");
        assert!(super::valid_identifier(&one));
        assert_ne!(one, super::identifier());
        assert_ne!(
            one,
            super::development_identifier("D:/Projects/Argus-worktree/src-gpui")
        );
        assert_eq!(
            one,
            super::development_identifier("D:/Projects/Argus/src-gpui")
        );
        if cfg!(windows) {
            assert_eq!(
                one,
                super::development_identifier("d:\\projects\\argus\\src-gpui")
            );
        }
    }
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
