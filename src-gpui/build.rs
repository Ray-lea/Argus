#[path = "../build-support/app-resource.rs"]
mod app_resource;
#[path = "../build-support/build-version.rs"]
mod build_version;
#[path = "../build-support/windows-runtime.rs"]
mod windows_runtime;

fn main() {
    windows_runtime::configure();
    let version = build_version::detect();
    println!("cargo:rustc-env=ARGUS_VERSION={}", version.full);
    println!("cargo:rustc-env=ARGUS_BUILD_NUMBER={}", version.build);
    app_resource::configure(&version.full, version.build);
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    println!("cargo:rerun-if-env-changed=ARGUS_BUILD_ASSETS");
    let assets = std::env::var_os("ARGUS_BUILD_ASSETS")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            let profile = std::env::var("PROFILE").ok()?;
            (profile == "release").then(|| {
                // Cargo 1.84+ nests the build directory as
                // `build/<package>/<hash>`, older cargo as
                // `build/<package>-<hash>`; both sit in the profile
                // directory, named by the profile.
                out.ancestors()
                    .find(|dir| dir.file_name().is_some_and(|name| *name == *profile))
                    .expect("Cargo profile directory")
                    .to_owned()
            })
        });
    let code = if let Some(path) = assets {
        let launcher = path.join("argus-taskmgr-launcher.exe");
        let recover = path.join("argus-taskmgr-recover.exe");
        assert!(
            launcher.is_file() && recover.is_file(),
            "task manager helpers must be built first"
        );
        println!("cargo:rerun-if-changed={}", launcher.display());
        println!("cargo:rerun-if-changed={}", recover.display());
        format!(
            "const ASSETS: Option<(&[u8], &[u8])> = Some((include_bytes!({:?}), include_bytes!({:?})));",
            launcher, recover
        )
    } else {
        "const ASSETS: Option<(&[u8], &[u8])> = None;".into()
    };
    std::fs::write(out.join("taskmgr-assets.rs"), code).unwrap();
}
