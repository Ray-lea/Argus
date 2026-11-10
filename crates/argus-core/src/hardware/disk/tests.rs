use super::*;

pub(super) struct FileFixture {
    pub(super) path: std::path::PathBuf,
}
impl FileFixture {
    pub(super) fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let next = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "argus-disk-removal-{}-{next}.tmp",
            std::process::id()
        ));
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        file.write_all(b"disk handle fixture").unwrap();
        Self { path }
    }
}
impl Drop for FileFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
        let _ = std::fs::remove_file(self.path.with_extension("moved"));
    }
}

fn disk(index: u32) -> Disk {
    Disk {
        handle: None,
        generation: 0,
        info: Arc::new(DiskInfo {
            index,
            model: format!("Disk {index}"),
            size: 1024,
            bus: "nvme",
            ssd: Some(true),
            removable: false,
            volumes: vec![],
        }),
        prev: None,
        temperature: Some((37., None)),
        temperature_at: None,
        temperature_supported: true,
    }
}
fn counters(second: i64, bytes: i64) -> DISK_PERFORMANCE {
    DISK_PERFORMANCE {
        QueryTime: second * 10_000_000,
        IdleTime: second * 5_000_000,
        BytesRead: bytes,
        BytesWritten: bytes * 2,
        ReadCount: second as u32,
        WriteCount: second as u32,
        ..Default::default()
    }
}
#[test]
fn unsupported_performance_keeps_both_disks_and_independent_temperature() {
    let mut tracker = DiskTracker {
        disks: vec![disk(0), disk(1)],
        scanned_at: Instant::now(),
    };
    tracker.collect(|d| (d.info.index == 1).then(|| counters(1, 100)));
    let readings = tracker.collect(|d| (d.info.index == 1).then(|| counters(2, 300)));
    assert_eq!(
        readings.iter().map(|d| d.info.index).collect::<Vec<_>>(),
        [0, 1]
    );
    assert!(
        readings[0].active.is_none()
            && readings[0].read.is_none()
            && readings[0].response_ms.is_none()
    );
    assert_eq!(readings[0].temperature, Some(37.));
    assert_eq!(readings[1].active, Some(50.));
    assert_eq!(readings[1].read, Some(200));
    let json = serde_json::to_value(&readings).unwrap();
    assert!(json[0]["active"].is_null());
    assert_eq!(json[0]["info"]["size"], 1024);
}
#[test]
fn a_counter_outage_or_reset_reprimes_without_cross_gap_rates() {
    let mut tracker = DiskTracker {
        disks: vec![disk(0)],
        scanned_at: Instant::now(),
    };
    assert_eq!(tracker.collect(|_| Some(counters(1, 100)))[0].read, None);
    assert_eq!(
        tracker.collect(|_| Some(counters(2, 300)))[0].read,
        Some(200)
    );
    assert_eq!(tracker.collect(|_| None)[0].read, None);
    assert_eq!(tracker.collect(|_| Some(counters(10, 10000)))[0].read, None);
    assert_eq!(
        tracker.collect(|_| Some(counters(11, 10100)))[0].read,
        Some(100)
    );
    assert_eq!(tracker.collect(|_| Some(counters(12, 10)))[0].read, None);
    assert_eq!(tracker.collect(|_| Some(counters(13, 10)))[0].read, Some(0));
}

#[test]
fn removal_and_cancellation_between_samples_reset_the_rate_baseline() {
    use windows::Win32::Devices::DeviceAndDriverInstallation::{
        CM_NOTIFY_ACTION_DEVICEQUERYREMOVE, CM_NOTIFY_ACTION_DEVICEQUERYREMOVEFAILED,
    };
    let file = FileFixture::new();
    let mut d = disk(42);
    d.handle = Some(DeviceHandle::fixture(&file.path));
    let mut tracker = DiskTracker {
        disks: vec![d],
        scanned_at: Instant::now(),
    };
    tracker.collect(|d| d.sample_with(|_| Ok(counters(1, 100))));
    let handle = tracker.disks[0].handle.as_ref().unwrap();
    handle.notify(CM_NOTIFY_ACTION_DEVICEQUERYREMOVE);
    handle.notify(CM_NOTIFY_ACTION_DEVICEQUERYREMOVEFAILED);
    let resumed = tracker.collect(|d| d.sample_with(|_| Ok(counters(9, 900))));
    assert!(resumed[0].read.is_none());
    let next = tracker.collect(|d| d.sample_with(|_| Ok(counters(10, 1100))));
    assert_eq!(next[0].read, Some(200));
}

#[test]
fn enumeration_pauses_during_query_remove_and_retirement_drops_the_disk() {
    use windows::Win32::Devices::DeviceAndDriverInstallation::{
        CM_NOTIFY_ACTION_DEVICEQUERYREMOVE, CM_NOTIFY_ACTION_DEVICEREMOVEPENDING,
    };
    let file = FileFixture::new();
    let mut d = disk(42);
    d.handle = Some(DeviceHandle::fixture(&file.path));
    let mut tracker = DiskTracker {
        disks: vec![d],
        scanned_at: Instant::now() - RESCAN_EVERY * 2,
    };
    tracker.disks[0]
        .handle
        .as_ref()
        .unwrap()
        .notify(CM_NOTIFY_ACTION_DEVICEQUERYREMOVE);
    let sample = tracker.sample();
    assert_eq!(sample.len(), 1);
    assert_eq!(sample[0].info.index, 42);
    assert!(sample[0].read.is_none());
    tracker.disks[0]
        .handle
        .as_ref()
        .unwrap()
        .notify(CM_NOTIFY_ACTION_DEVICEREMOVEPENDING);
    assert!(tracker.sample().is_empty());
    assert!(tracker.scanned_at.elapsed() < RESCAN_EVERY);
}
#[test]
fn empty_media_is_distinct_from_an_unavailable_internal_drive() {
    let mut info = (*disk(0).info).clone();
    info.size = 0;
    assert!(!no_media(&info, Some(21)));
    assert!(!no_media(&info, Some(50)));
    info.removable = true;
    assert!(no_media(&info, Some(21)));
    assert!(no_media(&info, Some(1112)));
    info.volumes.push("E:".into());
    assert!(!no_media(&info, Some(21)));
}
#[test]
#[ignore = "read-only device inventory using current rights; never elevates or enables diskperf settings"]
fn live_inventory() {
    crate::diagnostics::capture_helper_failures();
    let at = Instant::now();
    let mut tracker = DiskTracker::new();
    println!("disk inventory initialization: {:?}", at.elapsed());
    for round in 0..3 {
        if round == 2 {
            let at = Instant::now();
            tracker.rescan();
            println!("disk rescan: {:?}", at.elapsed());
        }
        let at = Instant::now();
        let sample = tracker.sample();
        println!(
            "disk sample: {:?}; {}",
            at.elapsed(),
            serde_json::to_string(&sample).unwrap()
        );
        std::thread::sleep(Duration::from_millis(600));
    }
    for entry in crate::diagnostics::take_helper_failures() {
        println!("{entry}");
    }
}

#[test]
#[ignore = "read-only device queries and simulated notifications; never requests device ejection"]
fn live_removal_cancellation() {
    use windows::Win32::Devices::DeviceAndDriverInstallation::{
        CM_NOTIFY_ACTION_DEVICEQUERYREMOVE, CM_NOTIFY_ACTION_DEVICEQUERYREMOVEFAILED,
    };
    let mut tracker = DiskTracker::new();
    let registered: Vec<_> = tracker
        .disks
        .iter()
        .filter(|d| d.handle.as_ref().is_some_and(DeviceHandle::registered))
        .map(|d| d.info.index)
        .collect();
    assert!(!registered.is_empty());
    tracker.sample();
    for disk in &tracker.disks {
        if registered.contains(&disk.info.index) {
            disk.handle
                .as_ref()
                .unwrap()
                .notify(CM_NOTIFY_ACTION_DEVICEQUERYREMOVE);
        }
    }
    let paused = tracker.sample();
    assert!(
        paused
            .iter()
            .filter(|d| registered.contains(&d.info.index))
            .all(|d| d.read.is_none() && d.temperature.is_none())
    );
    for disk in &tracker.disks {
        if registered.contains(&disk.info.index) {
            disk.handle
                .as_ref()
                .unwrap()
                .notify(CM_NOTIFY_ACTION_DEVICEQUERYREMOVEFAILED);
        }
    }
    let resumed = tracker.sample();
    assert!(
        resumed
            .iter()
            .filter(|d| registered.contains(&d.info.index))
            .all(|d| d.read.is_none())
    );
    std::thread::sleep(Duration::from_millis(100));
    let next = tracker.sample();
    assert_eq!(next.len(), paused.len());
    println!(
        "registered disks: {}; resumed counter streams: {}",
        registered.len(),
        next.iter().filter(|d| d.read.is_some()).count()
    );
}
