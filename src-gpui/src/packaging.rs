//! Installer-only commands, handled before settings, collection or window creation.
pub fn run_mode() -> Option<i32> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--argus-compare-version") => Some(
            match args.get(1).and_then(|s| semver::Version::parse(s).ok()) {
                Some(old) => match semver::Version::parse(env!("CARGO_PKG_VERSION"))
                    .unwrap()
                    .cmp(&old)
                {
                    std::cmp::Ordering::Equal => 0,
                    std::cmp::Ordering::Greater => 1,
                    std::cmp::Ordering::Less => 2,
                },
                None => 1,
            },
        ),
        Some("--argus-launch-installed") => {
            if args
                .iter()
                .skip(1)
                .any(|s| !matches!(s.as_str(), "--updated" | "--hidden"))
            {
                return Some(87);
            }
            let Ok(exe) = std::env::current_exe() else {
                return Some(2);
            };
            let mut command = argus_app::tasks::quote(&exe.to_string_lossy());
            for arg in &args[1..] {
                command.push(' ');
                command.push_str(arg);
            }
            Some(
                argus_app::tasks::launch(0, command, false)
                    .err()
                    .map_or(0, |error| error.code as i32),
            )
        }
        _ => None,
    }
}
