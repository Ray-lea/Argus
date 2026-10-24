//! Isolated VM smoke fixture for the administrator startup transport.
//! Never run management modes on a developer's desktop.
//! Build with `--release`; development hosts cannot register persistent grants.
const ID: &str = "dev.argus.admin-transport-test";
fn main() {
    argus_service::admin::configure_support(!cfg!(debug_assertions));
    if let Some(code) = argus_service::admin::run_mode(ID) {
        std::process::exit(code);
    }
    let args: Vec<_> = std::env::args().collect();
    if args.iter().any(|a| a == "status") {
        std::fs::write(
            std::env::temp_dir().join("argus-admin-transport-status.json"),
            serde_json::to_vec(&argus_service::admin::status(ID)).unwrap(),
        )
        .unwrap();
        return;
    }
    if !argus_core::privilege::is_elevated()
        && argus_service::admin::enabled(ID)
        && !args.iter().any(|a| a == argus_service::admin::FALLBACK)
    {
        if let Err(error) = argus_service::admin::prepare(ID, &args, true).and_then(|p| p.commit())
        {
            std::fs::write(
                std::env::temp_dir().join("argus-admin-transport-error.txt"),
                error,
            )
            .unwrap();
            std::process::exit(1);
        }
        return;
    }
    let path = std::env::temp_dir().join("argus-admin-transport-result.json");
    std::fs::write(
        path,
        serde_json::to_vec(&serde_json::json!({
            "pid": std::process::id(), "elevated": argus_core::privilege::is_elevated(),
            "hidden": args.iter().any(|a| a == "--hidden"),
            "updated": args.iter().any(|a| a == "--updated"),
        }))
        .unwrap(),
    )
    .unwrap();
    argus_service::admin::acknowledge(&args);
}
