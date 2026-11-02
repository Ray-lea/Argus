//! A native entry point for the cadence probe, independent of shell associations.
use std::fs::{self, File, OpenOptions};
use std::io::{self, IsTerminal, Write};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use windows::Win32::System::Threading::CREATE_NO_WINDOW;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub fn run() -> Result<()> {
    let result = run_inner();
    if let Err(error) = &result {
        eprintln!("Network diagnostic stopped: {error}");
    }
    if io::stdin().is_terminal() && !std::env::args().any(|arg| arg == "--no-wait") {
        println!("Press Enter to close this window.");
        let _ = io::stdin().read_line(&mut String::new());
    }
    result
}

fn output_root(exe: &Path) -> io::Result<(PathBuf, File)> {
    let beside = exe.parent().unwrap_or(Path::new("."));
    let fallback = std::env::temp_dir().join("Argus-network-diagnostic");
    for root in [beside, fallback.as_path()] {
        if fs::create_dir_all(root).is_err() {
            continue;
        }
        if let Ok(log) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(root.join("network-launcher.log"))
        {
            return Ok((root.to_path_buf(), log));
        }
    }
    Err(io::Error::new(
        io::ErrorKind::PermissionDenied,
        "diagnostic output directory",
    ))
}

fn report(log: &mut File, text: &str) -> io::Result<()> {
    println!("{text}");
    writeln!(log, "{text}")?;
    log.flush()
}

fn run_inner() -> Result<()> {
    let exe = std::env::current_exe()?;
    let (root, mut log) = output_root(&exe)?;
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();
    report(
        &mut log,
        &format!("START {stamp}; pid={}", std::process::id()),
    )?;
    let out = root.join(format!("network-results-{stamp}"));
    fs::create_dir(&out)?;
    report(&mut log, &format!("Output: {}", out.display()))?;
    let elevated = argus_core::privilege::is_elevated();
    report(&mut log, &format!("Administrator: {elevated}"))?;
    let mut results = Vec::new();
    fs::write(
        out.join("environment.json"),
        serde_json::to_vec_pretty(&json!({
            "startedUnixMs":stamp,
            "elevated":elevated,
            "probeSha256":Sha256::digest(fs::read(&exe)?).iter().map(|byte| format!("{byte:02x}")).collect::<String>(),
        }))?,
    )?;
    for (name, args) in [
        ("service-fresh-1000", vec!["1000", "17", "--collector"]),
        (
            "local-fresh-1000",
            vec!["1000", "17", "--collector", "--local"],
        ),
        (
            "local-fresh-500",
            vec!["500", "17", "--collector", "--local"],
        ),
        (
            "local-raw-1000",
            vec!["1000", "17", "--local", "--raw-local"],
        ),
    ] {
        report(&mut log, &format!("Running {name}"))?;
        let stdout = out.join(format!("{name}.jsonl"));
        let stderr = out.join(format!("{name}.stderr.txt"));
        let mut child = match Command::new(&exe)
            .args(&args)
            .stdin(Stdio::null())
            .stdout(File::create(&stdout)?)
            .stderr(File::create(&stderr)?)
            .creation_flags(CREATE_NO_WINDOW.0)
            .spawn()
        {
            Ok(child) => child,
            Err(error) => {
                let failure = json!({"name":name,"spawnError":error.raw_os_error()});
                report(&mut log, &failure.to_string())?;
                results.push(failure);
                continue;
            }
        };
        let pid = child.id();
        let started = Instant::now();
        let (status, timed_out) = loop {
            if let Some(status) = child.try_wait()? {
                break (status, false);
            }
            if started.elapsed() > Duration::from_secs(60) {
                child.kill()?;
                break (child.wait()?, true);
            }
            std::thread::sleep(Duration::from_millis(100));
        };
        if args.contains(&"--local") && (!status.success() || timed_out) {
            // A terminated child cannot drop its own ETW tracker.
            let prefix = if args.contains(&"--collector") {
                "Argus Network Collector Test"
            } else {
                "Argus Network Cadence Test"
            };
            argus_core::net_process::stop_session(&format!("{prefix} {pid}"));
        }
        let rows: Vec<Value> = fs::read_to_string(stdout)?
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect();
        let active: Vec<_> = rows
            .iter()
            .filter(|row| {
                row["event"] == "sample"
                    && row["atMs"]
                        .as_u64()
                        .is_some_and(|ms| (5000..=11000).contains(&ms))
            })
            .collect();
        let result = json!({
            "name":name,"exitCode":status.code(),"timedOut":timed_out,
            "activeSamples":active.len(),
            "availableSamples":active.iter().filter(|row| row["available"] == true).count(),
            "zeroSamples":active.iter().filter(|row| row["sentRate"] == 0 && row["receivedRate"] == 0).count(),
            "localUnavailable":rows.iter().any(|row| row["event"] == "local-unavailable"),
        });
        report(&mut log, &result.to_string())?;
        results.push(result);
        fs::write(
            out.join("summary.json"),
            serde_json::to_vec_pretty(&results)?,
        )?;
    }
    fs::write(
        out.join("summary.json"),
        serde_json::to_vec_pretty(&results)?,
    )?;
    report(&mut log, &format!("COMPLETE: {}", out.display()))?;
    Ok(())
}
