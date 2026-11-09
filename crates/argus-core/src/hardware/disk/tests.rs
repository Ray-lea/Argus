use super::*;

fn disk(index: u32) -> Disk {
    Disk {
        handle: None,
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
