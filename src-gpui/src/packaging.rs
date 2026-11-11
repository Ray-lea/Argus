//! Installer-only commands, handled before settings, collection or window creation.
pub fn run_mode() -> Option<i32> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--argus-build-info")
            if args.len() == 1
                || matches!(args.as_slice(), [_, hidden, test, flag, id]
            if hidden == "--hidden" && test == "--test" && flag == "--identifier"
                && id.starts_with("dev.argus.taskmanager.test.") && argus_app::config::valid_identifier(id)) =>
        {
            println!(
                "{}",
                serde_json::json!({
                    "version": env!("CARGO_PKG_VERSION"),
                    "build": env!("ARGUS_VERSION"),
                    "buildKind": argus_app::config::build_kind(env!("ARGUS_VERSION"), cfg!(debug_assertions)),
                    "fileVersion": format!("{}.{}", env!("CARGO_PKG_VERSION").split(['-', '+']).next().unwrap(), env!("ARGUS_BUILD_NUMBER")),
                })
            );
            Some(0)
        }
        Some("--argus-build-info") => Some(87),
        Some("--argus-compare-version") => Some(compare_version(
            env!("CARGO_PKG_VERSION"),
            args.get(1).map(String::as_str),
        )),
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
                argus_app::tasks::launch_installed(command)
                    .err()
                    .map_or(0, |error| error.code as i32),
            )
        }
        _ => None,
    }
}

fn compare_version(current: &str, installed: Option<&str>) -> i32 {
    match installed.and_then(|value| semver::Version::parse(value).ok()) {
        Some(installed) => match semver::Version::parse(current)
            .unwrap()
            .cmp_precedence(&installed)
        {
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Greater => 1,
            std::cmp::Ordering::Less => 2,
        },
        None => 1,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn installer_compares_release_precedence_without_build_metadata() {
        use super::compare_version;
        assert_eq!(compare_version("0.4.0", Some("0.4.0+672.g1234567")), 0);
        assert_eq!(
            compare_version("0.4.0+673.g7654321", Some("0.4.0+672.g1234567")),
            0
        );
        assert_eq!(compare_version("0.4.1", Some("0.4.0+672.g1234567")), 1);
        assert_eq!(compare_version("0.4.1-beta.1", Some("0.4.1")), 2);
        assert_eq!(compare_version("0.4.1", Some("0.4.1-beta.1")), 1);
    }
}
