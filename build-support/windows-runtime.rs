//! Links the VC++ runtime statically into application and helper programs.
//! Task manager helpers must start without the VC++ redistributable so the
//! replacement entry point remains available. The Universal CRT stays dynamic
//! because Windows supplies it. Adapted from tauri-build's static_vcruntime
//! implementation (ChrisDenton/static_vcruntime); this is source attribution.

use std::path::Path;

pub fn configure() {
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("msvc") {
        return;
    }
    override_msvcrt_lib();
    for lib in [
        "libvcruntimed.lib",
        "vcruntime.lib",
        "vcruntimed.lib",
        "libcmtd.lib",
        "msvcrt.lib",
        "msvcrtd.lib",
        "libucrt.lib",
        "libucrtd.lib",
    ] {
        println!("cargo:rustc-link-arg=/NODEFAULTLIB:{lib}");
    }
    for lib in ["libcmt.lib", "libvcruntime.lib", "ucrt.lib"] {
        println!("cargo:rustc-link-arg=/DEFAULTLIB:{lib}");
    }
}

/// Rust always asks for msvcrt.lib; an empty one in the search path stands in.
fn override_msvcrt_lib() {
    let machine: &[u8] = match std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
        Ok("x86_64") => &[0x64, 0x86],
        Ok("x86") => &[0x4C, 0x01],
        _ => return,
    };
    let empty_object: &[u8] = &[
        1, 0, 94, 3, 96, 98, 60, 0, 0, 0, 1, 0, 0, 0, 0, 0, 132, 1, 46, 100, 114, 101, 99, 116,
        118, 101, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 60, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 10, 16, 0, 46, 100, 114, 101, 99, 116, 118, 101, 0, 0, 0, 0, 1, 0, 0, 0, 3, 0, 4, 0,
        0, 0,
    ];
    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR is set for build scripts");
    let lib = Path::new(&out_dir).join("msvcrt.lib");
    std::fs::write(&lib, [machine, empty_object].concat()).expect("write msvcrt.lib");
    println!("cargo:rustc-link-search=native={out_dir}");
}
