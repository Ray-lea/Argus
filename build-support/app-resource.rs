use std::{env, fs, path::PathBuf};

pub fn configure() {
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("..");
    let manifest = root.join("resources/app.json");
    let metadata: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let version = env::var("CARGO_PKG_VERSION").unwrap();
    let numeric = version.split('-').next().unwrap().replace('.', ",") + ",0";
    let quote = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
    let icon = root
        .join("resources/icons/icon.ico")
        .canonicalize()
        .unwrap();
    let resource = format!(
        "#pragma code_page(65001)\n#include <windows.h>\n1 ICON \"{}\"\n1 VERSIONINFO\nFILEVERSION {}\nPRODUCTVERSION {}\nFILEOS VOS_NT_WINDOWS32\nFILETYPE VFT_APP\nBEGIN\n BLOCK \"StringFileInfo\"\n BEGIN\n BLOCK \"040904B0\"\n BEGIN\n VALUE \"ProductName\", \"{}\\0\"\n VALUE \"FileDescription\", \"{}\\0\"\n VALUE \"FileVersion\", \"{}\\0\"\n VALUE \"ProductVersion\", \"{}\\0\"\n VALUE \"CompanyName\", \"{}\\0\"\n VALUE \"LegalCopyright\", \"{}\\0\"\n END\n END\n BLOCK \"VarFileInfo\"\n BEGIN\n VALUE \"Translation\", 0x409, 1200\n END\nEND\n",
        quote(&icon.display().to_string()),
        numeric,
        numeric,
        quote(metadata["productName"].as_str().unwrap()),
        quote(metadata["productName"].as_str().unwrap()),
        version,
        version,
        quote(metadata["publisher"].as_str().unwrap()),
        quote(metadata["copyright"].as_str().unwrap())
    );
    let file = out.join("app.rc");
    fs::write(&file, resource).unwrap();
    for path in [manifest, icon] {
        println!("cargo:rerun-if-changed={}", path.display());
    }
    embed_resource::compile_for(file, ["argus"], embed_resource::NONE)
        .manifest_required()
        .expect("compile native application resources");
}
