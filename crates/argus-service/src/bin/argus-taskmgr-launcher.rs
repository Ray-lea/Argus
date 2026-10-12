#![windows_subsystem = "windows"]
fn main() {
    std::process::exit(argus_service::taskmgr::launcher_main());
}
