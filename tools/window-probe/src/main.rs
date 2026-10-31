#[cfg(feature = "capture")]
use anyhow::Context as _;

#[cfg(all(windows, feature = "capture"))]
mod frames;
#[cfg(windows)]
mod latency;
#[cfg(feature = "capture")]
mod pixels;
#[cfg(all(windows, feature = "capture"))]
mod record;
#[cfg(all(windows, target_arch = "x86_64"))]
mod stacks;
#[cfg(windows)]
mod windows;

fn unix_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        #[cfg(feature = "capture")]
        Some("compare") => pixels::compare(&args[1], &args[2])?,
        #[cfg(feature = "capture")]
        Some("frame-contrast") => {
            pixels::frame_contrast(args.get(1).context("recording directory required")?)?
        }
        #[cfg(feature = "capture")]
        Some("track-chart") => {
            pixels::track_chart(args.get(1).context("recording directory required")?)?
        }
        #[cfg(windows)]
        Some("run") => windows::run(&args[1..])?,
        #[cfg(windows)]
        Some("inspect") => windows::inspect(args[1].parse()?)?,
        #[cfg(windows)]
        Some("startup") => windows::startup(&args[1..])?,
        #[cfg(windows)]
        Some("latency") => latency::run(&args[1], args.get(2).map_or(Ok(20), |s| s.parse())?)?,
        #[cfg(all(windows, feature = "capture"))]
        Some("capture") => windows::capture(args[1].parse()?, &args[2])?,
        _ => anyhow::bail!(
            "usage: argus-window-probe run EXE [ARGS] | capture PID PNG | compare LEFT RIGHT | frame-contrast DIRECTORY | track-chart DIRECTORY"
        ),
    }
    Ok(())
}
