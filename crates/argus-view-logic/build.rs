// The tests link like the application: `argus-app` puts an empty msvcrt.lib
// in the search path of everything that depends on it, so the runtime has to
// be named here as well.
#[path = "../../build-support/windows-runtime.rs"]
mod windows_runtime;

fn main() {
    windows_runtime::configure();
}
