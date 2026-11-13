//! Real recording on/off cycle in a newly created temporary folder, without rules or settings.
use argus_app::{
    channel::Channel,
    sampler::{Control, Sampler},
};
use argus_core::{Collector, MetaStore, history};
use std::{
    path::Path,
    sync::{Arc, mpsc},
    time::{Duration, Instant},
};

fn subscribe(sampler: &Sampler) -> mpsc::Receiver<u64> {
    let (tx, rx) = mpsc::channel();
    sampler.send(Control::Subscribe(Channel::new(
        move |snapshot: argus_core::Snapshot| {
            tx.send(snapshot.seq)?;
            Ok(())
        },
    )));
    rx
}

fn await_frame(rx: &mpsc::Receiver<u64>, dir: &Path, bytes: u64) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while history::usage(dir).bytes <= bytes || history::range(dir).is_none() {
        rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(Instant::now() < deadline, "no complete recorded frame");
    }
}

#[test]
#[ignore = "real Windows collector and recording lifecycle, run explicitly with --ignored"]
fn disabling_history_stops_writes_while_sampling_and_reenable_preserves_old_frames() {
    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "argus-history-lifecycle-{}-{suffix}",
        std::process::id()
    ));
    std::fs::create_dir(&dir).unwrap();
    let sampler = Sampler::spawn(
        || Collector::new(MetaStore::new()),
        Arc::new(argus_service::RuleRunner::spawn(false)),
    );
    sampler.send(Control::SetInterval(Duration::from_millis(500)));
    sampler.send(Control::SetHistoryDir(Some(dir.clone())));
    let rx = subscribe(&sampler);
    await_frame(&rx, &dir, 5);
    sampler.send(Control::SetHistoryDir(None));
    // A new subscription's first callback is a FIFO barrier after SetHistoryDir.
    let rx = subscribe(&sampler);
    let first = rx.recv_timeout(Duration::from_secs(10)).unwrap();
    let stopped_bytes = history::usage(&dir).bytes;
    let stopped_range = history::range(&dir).unwrap();
    let until = Instant::now() + Duration::from_millis(history::FRAME_MS as u64 + 1000);
    let mut last = first;
    while Instant::now() < until {
        last = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(history::usage(&dir).bytes, stopped_bytes);
    }
    assert!(
        last > first + 3,
        "collector did not continue while recording was disabled"
    );
    assert_eq!(history::range(&dir).unwrap(), stopped_range);
    sampler.send(Control::SetHistoryDir(Some(dir.clone())));
    let rx = subscribe(&sampler);
    await_frame(&rx, &dir, stopped_bytes);
    let resumed = history::range(&dir).unwrap();
    assert_eq!(resumed.0, stopped_range.0);
    assert!(resumed.1 > stopped_range.1);
    sampler.send(Control::SetHistoryDir(None));
    subscribe(&sampler)
        .recv_timeout(Duration::from_secs(10))
        .unwrap();
    let (tx, rx) = mpsc::channel();
    sampler.send(Control::ClearHistory(dir.clone(), tx));
    rx.recv_timeout(Duration::from_secs(10)).unwrap().unwrap();
    assert_eq!(history::usage(&dir).bytes, 0);
    drop(sampler);
    std::fs::remove_dir(dir).unwrap();
}
