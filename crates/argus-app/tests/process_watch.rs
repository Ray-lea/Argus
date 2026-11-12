//! Real sampler integration; only observes this test process and writes its own temp folder.
use argus_app::{
    channel::Channel,
    sampler::{Control, Sampler},
};
use argus_core::{
    Collector, MetaStore,
    watch::{WatchAction, WatchError, WatchMode, WatchRecorder, WatchState, WatchView},
};
use std::sync::{Arc, mpsc};
use std::time::Duration;

fn request(
    sampler: &Sampler,
    dir: &std::path::Path,
    action: WatchAction,
) -> Result<Option<WatchView>, WatchError> {
    let (tx, rx) = mpsc::channel();
    sampler.send(Control::Watch(dir.to_owned(), action, tx));
    rx.recv_timeout(Duration::from_secs(15)).unwrap()
}

#[test]
#[ignore = "real Windows collector integration, run explicitly with --ignored"]
fn observes_and_deletes_through_the_real_sampler() {
    let dir = std::env::temp_dir().join(format!("argus-sampler-watch-{}", std::process::id()));
    let rules = Arc::new(argus_service::RuleRunner::spawn(false));
    // No rules are installed and no service operations or system settings are changed.
    let sampler = Sampler::spawn(|| Collector::new(MetaStore::new()), rules);
    let (tx, rx) = mpsc::channel();
    sampler.send(Control::Subscribe(Channel::new(move |snapshot| {
        tx.send(snapshot)?;
        Ok(())
    })));
    let snapshot = rx.recv_timeout(Duration::from_secs(30)).unwrap();
    let own = snapshot
        .processes
        .iter()
        .find(|p| p.pid == std::process::id())
        .unwrap();
    let start = WatchAction::Start {
        pid: own.pid,
        create_time: own.create_time,
        name: "sampler-owned-test".into(),
    };
    request(&sampler, &dir, start).unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    loop {
        let _ = rx.recv_timeout(Duration::from_secs(10)).unwrap();
        let view = request(&sampler, &dir, WatchAction::Status)
            .unwrap()
            .unwrap();
        if view.points.iter().any(|point| point.memory.is_some()) {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "no valid observation through sampler"
        );
    }
    assert_eq!(
        request(&sampler, &dir, WatchAction::Stop)
            .unwrap()
            .unwrap()
            .state,
        WatchState::Stopped
    );
    assert!(
        request(&sampler, &dir, WatchAction::Delete)
            .unwrap()
            .is_none()
    );
    assert!(!dir.join("process-watch.jsonl").exists());
    request(
        &sampler,
        &dir,
        WatchAction::Start {
            pid: own.pid,
            create_time: own.create_time,
            name: "sampler-owned-test".into(),
        },
    )
    .unwrap();
    let (clear_tx, clear_rx) = mpsc::channel();
    sampler.send(Control::ClearHistory(dir.clone(), clear_tx));
    clear_rx
        .recv_timeout(Duration::from_secs(15))
        .unwrap()
        .unwrap();
    assert!(
        request(&sampler, &dir, WatchAction::Status)
            .unwrap()
            .is_none()
    );
    request(
        &sampler,
        &dir,
        WatchAction::StartProgram {
            pid: own.pid,
            create_time: own.create_time,
            name: "sampler-owned-program".into(),
        },
    )
    .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    loop {
        let _ = rx.recv_timeout(Duration::from_secs(10)).unwrap();
        let view = request(&sampler, &dir, WatchAction::Status)
            .unwrap()
            .unwrap();
        assert_eq!(view.mode, WatchMode::Program);
        if view.points.iter().any(|point| point.memory.is_some()) {
            break;
        }
        assert!(std::time::Instant::now() < deadline);
    }
    let stopped = request(&sampler, &dir, WatchAction::Stop).unwrap().unwrap();
    assert_eq!(stopped.state, WatchState::Stopped);
    assert_eq!(stopped.live_runs, 0);
    assert!(WatchRecorder::storage_bytes(&dir) > 0);
    request(&sampler, &dir, WatchAction::Delete).unwrap();
    assert_eq!(WatchRecorder::storage_bytes(&dir), 0);
    request(
        &sampler,
        &dir,
        WatchAction::StartProgram {
            pid: own.pid,
            create_time: own.create_time,
            name: "sampler-owned-program".into(),
        },
    )
    .unwrap();
    let (clear_tx, clear_rx) = mpsc::channel();
    sampler.send(Control::ClearHistory(dir.clone(), clear_tx));
    clear_rx
        .recv_timeout(Duration::from_secs(15))
        .unwrap()
        .unwrap();
    assert!(
        request(&sampler, &dir, WatchAction::Status)
            .unwrap()
            .is_none()
    );
    assert_eq!(WatchRecorder::storage_bytes(&dir), 0);
    drop(sampler);
    drop(rx);
    std::fs::remove_dir(dir).unwrap();
}
