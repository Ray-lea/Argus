//! The version a build shows. A release build (its commit tagged
//! `v<version>`) shows the package version alone; any other build adds
//! SemVer build metadata naming its commit: `0.3.4+583.g4c638d7`, the number
//! of commits up to it and its abbreviated hash. Build metadata takes no part
//! in version precedence, so a test build still compares as the release it
//! follows: comparisons (updates, the privileged service, the first start of
//! a version) keep the package version. `ARGUS_BUILD_METADATA` sets the
//! metadata instead; empty leaves it out.
use std::{
    env,
    path::{Path, PathBuf},
    process::Command,
};

pub struct BuildVersion {
    /// `0.3.4`, or `0.3.4+583.g4c638d7`.
    pub full: String,
    /// The number of commits, for the last part of the numeric file version
    /// (0 for a release build).
    pub build: u16,
}

pub fn detect() -> BuildVersion {
    let version = env::var("CARGO_PKG_VERSION").unwrap();
    let dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let git = |args: &[&str]| {
        Command::new("git")
            .args(args)
            .current_dir(&dir)
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
    };
    println!("cargo:rerun-if-env-changed=ARGUS_BUILD_METADATA");
    // Again when HEAD moves or a tag is made. Only paths that exist: Cargo
    // runs a build script that watches a missing file on every build.
    let head = git(&["symbolic-ref", "-q", "HEAD"]);
    for name in ["HEAD", "packed-refs", "refs/tags"]
        .into_iter()
        .chain(head.as_deref())
    {
        if let Some(path) = git(&["rev-parse", "--git-path", name]) {
            let path = dir.join(Path::new(&path));
            if path.exists() {
                println!("cargo:rerun-if-changed={}", path.display());
            }
        }
    }
    let count = git(&["rev-list", "--count", "HEAD"]).and_then(|count| count.parse::<u32>().ok());
    let metadata = match env::var("ARGUS_BUILD_METADATA") {
        Ok(metadata) => Some(metadata.trim().to_owned()).filter(|m| !m.is_empty()),
        Err(_) => {
            let release = git(&["tag", "--points-at", "HEAD"])
                .is_some_and(|tags| tags.lines().any(|tag| tag == format!("v{version}")));
            match (release, count, git(&["rev-parse", "--short=7", "HEAD"])) {
                (false, Some(count), Some(hash)) => Some(format!("{count}.g{hash}")),
                _ => None,
            }
        }
    };
    if let Some(metadata) = &metadata {
        assert!(
            metadata.split('.').all(|part| !part.is_empty()
                && part.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')),
            "build metadata must be dot-separated [0-9A-Za-z-] identifiers: {metadata}"
        );
    }
    BuildVersion {
        full: match &metadata {
            Some(metadata) => format!("{version}+{metadata}"),
            None => version,
        },
        build: metadata
            .and(count)
            .map_or(0, |count| count.min(u32::from(u16::MAX)) as u16),
    }
}
