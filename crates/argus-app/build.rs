#[path = "../../build-support/windows-runtime.rs"]
mod windows_runtime;

fn main() {
    windows_runtime::configure();
}
