#![windows_subsystem = "windows"]
fn main() {
    if let Some(code) = argus_service::admin::launcher_mode() {
        std::process::exit(code);
    }
    std::process::exit(argus_service::taskmgr::launcher_main());
}
