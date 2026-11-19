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
            history_id: String::new(),
            index,
            model: format!("Disk {index}"),
            size: 1024,
            bus: "nvme",
            ssd: Some(true),
            removable: false,
            volumes: vec![],
        }),
        controller: Some("stornvme".into()),
        prev: None,
        fallback: volumes::Counters::default(),
        fallback_reported: false,
        temperature: Some((37., None)),
        temperature_at: None,
        temperature_supported: true,
        temperature_transfers: None,
    }
}
pub(super) fn counters(second: i64, bytes: i64) -> DISK_PERFORMANCE {
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
fn invalid_function_uses_volume_counters_and_preserves_primary_recovery() {
    let file = FileFixture::new();
    let mut d = disk(0);
    Arc::make_mut(&mut d.info).volumes = vec!["E:".into()];
    d.handle = Some(DeviceHandle::fixture(&file.path));
    d.temperature_supported = false;
    let mut tracker = DiskTracker {
        disks: vec![d],
        scanned_at: Instant::now(),
    };
    let volume = |second, bytes| {
        let mut value = counters(second, bytes);
        value
            .StorageManagerName
            .copy_from_slice(&wide("VOLMGR  ")[..8]);
        value.StorageDeviceNumber = 6;
        volumes::Reading::Volume(value)
    };
    assert!(
        tracker.collect_with_fallback(|_| Err(1), |_, _| Ok(volume(1, 100)))[0]
            .read
            .is_none()
    );
    let next = tracker.collect_with_fallback(
        |_| Err(1),
        |index, path| {
            assert_eq!((index, path), (0, "E:"));
            Ok(volume(2, 300))
        },
    );
    assert_eq!(next[0].read, Some(200));
    assert_eq!(next[0].write, Some(400));
    assert_eq!(next[0].active, Some(50.));
    assert_eq!(next[0].temperature, Some(37.));
    assert_eq!(
        next[0].performance_volumes.as_deref(),
        Some(["E:".into()].as_slice())
    );
    let recovered = tracker.collect_with_fallback(
        |_| Ok(counters(3, 500)),
        |_, _| panic!("healthy device needs no fallback"),
    );
    assert!(recovered[0].read.is_none());
    assert!(recovered[0].performance_volumes.is_none());
    assert_eq!(
        tracker.collect_with_fallback(|_| Ok(counters(4, 900)), |_, _| unreachable!())[0].read,
        Some(400)
    );
}

#[test]
fn removal_and_volume_failures_do_not_read_other_sources_or_reuse_old_rates() {
    use windows::Win32::Devices::DeviceAndDriverInstallation::CM_NOTIFY_ACTION_DEVICEQUERYREMOVE;
    let file = FileFixture::new();
    let mut d = disk(0);
    Arc::make_mut(&mut d.info).volumes = vec!["E:".into()];
    d.handle = Some(DeviceHandle::fixture(&file.path));
    let mut tracker = DiskTracker {
        disks: vec![d],
        scanned_at: Instant::now(),
    };
    let missing = tracker.collect_with_fallback(|_| Err(1), |_, _| Err(1));
    assert!(missing[0].read.is_none());
    assert!(missing[0].active.is_none());
    tracker.disks[0]
        .handle
        .as_ref()
        .unwrap()
        .notify(CM_NOTIFY_ACTION_DEVICEQUERYREMOVE);
    assert!(
        tracker.collect_with_fallback(
            |_| panic!("read during removal"),
            |_, _| panic!("fallback during removal")
        )[0]
        .read
        .is_none()
    );
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
fn temperature_is_queried_only_on_buses_the_inbox_miniports_serve() {
    for (code, allowed) in [
        (0, false),
        (1, false), // SCSI
        (3, true),  // ATA
        (7, false), // USB
        (8, false), // RAID: Intel RST and VMD, vendor RAID
        (10, false),
        (11, true), // SATA
        (12, false),
        (13, false),
        (14, false),
        (15, false),
        (16, false),
        (17, true), // NVMe
        (99, false),
    ] {
        assert_eq!(
            temperature_query_allowed(bus_name(code), Some("stornvme")),
            allowed,
            "bus type {code}"
        );
    }
}

#[test]
fn temperature_is_queried_only_from_microsofts_own_storage_drivers() {
    for (bus, controller, allowed) in [
        ("nvme", Some("stornvme"), true),
        ("sata", Some("storahci"), true),
        ("ata", Some("atapi"), true),
        // Intel RST VMD passes NVMe drives through with the NVMe bus type,
        // and RST presents SATA drives it manages as SATA.
        ("nvme", Some("iaStorVD"), false),
        ("sata", Some("iaStorAC"), false),
        ("nvme", Some("secnvme"), false),
        // A disk whose driver PnP does not name is not asked either.
        ("nvme", None, false),
        ("sata", None, false),
        ("usb", Some("USBSTOR"), false),
    ] {
        assert_eq!(
            temperature_query_allowed(bus, controller),
            allowed,
            "{bus} behind {controller:?}"
        );
    }
}

#[test]
fn a_disk_behind_a_vendor_storage_driver_is_never_asked_for_its_temperature() {
    use windows::Win32::Devices::DeviceAndDriverInstallation::{
        CM_NOTIFY_ACTION_DEVICEQUERYREMOVE, CM_NOTIFY_ACTION_DEVICEQUERYREMOVEFAILED,
    };
    for (bus, controller) in [
        ("raid", Some("iaStorAC")),
        // What Intel RST VMD reports for an NVMe drive it passes through.
        ("nvme", Some("iaStorVD")),
        ("nvme", None),
    ] {
        let file = FileFixture::new();
        let mut info = (*disk(0).info).clone();
        info.bus = bus;
        let mut d = Disk::new(
            Some(DeviceHandle::fixture(&file.path)),
            info,
            controller.map(String::from),
        );
        let never = |_: HANDLE| -> Option<(f32, Option<f32>)> {
            panic!("temperature query sent to a vendor storage driver")
        };
        assert!(
            d.sample_with_queries(|_| Ok(counters(1, 100)), never)
                .is_some()
        );
        // Neither the periodic re-arm nor a reopened device handle starts asking.
        d.arm_temperature();
        d.temperature_at = None;
        assert!(
            d.sample_with_queries(|_| Ok(counters(2, 300)), never)
                .is_some()
        );
        {
            let handle = d.handle.as_ref().unwrap();
            handle.notify(CM_NOTIFY_ACTION_DEVICEQUERYREMOVE);
            handle.notify(CM_NOTIFY_ACTION_DEVICEQUERYREMOVEFAILED);
        }
        assert!(
            d.sample_with_queries(|_| Ok(counters(3, 500)), never)
                .is_some()
        );
        assert!(d.temperature.is_none() && !d.temperature_supported);
    }
}

/// Prints what this machine's disks are served by; run by hand.
#[test]
#[ignore = "reads the PnP tree and opens the disks of this machine"]
fn live_controllers() {
    let started = Instant::now();
    let found = discovery::disks();
    let elapsed = started.elapsed();
    let mut disks: Vec<_> = found.iter().collect();
    disks.sort_by_key(|(index, _)| **index);
    for (index, disk) in disks {
        println!(
            "disk {index}: storage driver={}",
            controller::label(disk.controller.as_deref())
        );
    }
    println!("discovery took {elapsed:?}");
}

#[test]
fn a_declining_nvme_disk_is_asked_again_only_after_a_rescan_or_reopen() {
    use windows::Win32::Devices::DeviceAndDriverInstallation::{
        CM_NOTIFY_ACTION_DEVICEQUERYREMOVE, CM_NOTIFY_ACTION_DEVICEQUERYREMOVEFAILED,
    };
    let file = FileFixture::new();
    let mut d = Disk::new(
        Some(DeviceHandle::fixture(&file.path)),
        (*disk(0).info).clone(),
        Some("stornvme".into()),
    );
    let mut asked = 0;
    d.sample_with_queries(
        |_| Ok(counters(1, 100)),
        |_| {
            asked += 1;
            None
        },
    );
    d.temperature_at = None;
    d.sample_with_queries(
        |_| Ok(counters(2, 300)),
        |_| {
            asked += 1;
            None
        },
    );
    assert_eq!(asked, 1);
    d.arm_temperature();
    d.temperature_at = None;
    d.sample_with_queries(
        |_| Ok(counters(3, 500)),
        |_| {
            asked += 1;
            Some((41., Some(70.)))
        },
    );
    assert_eq!(asked, 2);
    assert_eq!(d.temperature, Some((41., Some(70.))));
    {
        let handle = d.handle.as_ref().unwrap();
        handle.notify(CM_NOTIFY_ACTION_DEVICEQUERYREMOVE);
        handle.notify(CM_NOTIFY_ACTION_DEVICEQUERYREMOVEFAILED);
    }
    d.sample_with_queries(
        |_| Ok(counters(4, 700)),
        |_| {
            asked += 1;
            Some((42., None))
        },
    );
    assert_eq!(asked, 3);
    assert_eq!(d.temperature, Some((42., None)));
}

/// A disk on the inbox AHCI driver; `ssd` false makes it a hard disk.
fn sata_disk(file: &FileFixture, ssd: bool) -> Disk {
    let mut info = (*disk(0).info).clone();
    info.bus = "sata";
    info.ssd = Some(ssd);
    Disk::new(
        Some(DeviceHandle::fixture(&file.path)),
        info,
        Some("storahci".into()),
    )
}

/// One sample whose temperature query is due; counts the queries sent.
fn sample_power(
    d: &mut Disk,
    asked: &std::cell::Cell<u32>,
    read: Result<DISK_PERFORMANCE, u32>,
    working: bool,
) {
    d.temperature_at = None;
    d.sample_with_power(
        |_| read,
        |_| {
            asked.set(asked.get() + 1);
            Some((30. + asked.get() as f32, None))
        },
        |_| working,
    );
}

#[test]
fn a_powered_down_disk_is_not_asked_and_is_asked_once_after_it_wakes() {
    for ssd in [false, true] {
        let file = FileFixture::new();
        let mut d = sata_disk(&file, ssd);
        let asked = std::cell::Cell::new(0);
        sample_power(&mut d, &asked, Ok(counters(1, 100)), true);
        assert_eq!((asked.get(), d.temperature), (1, Some((31., None))));
        // Powered down: no query, and no reading of a drive that is off.
        for _ in 0..3 {
            sample_power(&mut d, &asked, Ok(counters(1, 100)), false);
            assert_eq!((asked.get(), d.temperature), (1, None));
            assert!(d.temperature_supported);
        }
        // Awake without a transfer counted, as after another tool's query.
        sample_power(&mut d, &asked, Ok(counters(1, 100)), true);
        assert_eq!((asked.get(), d.temperature), (2, Some((32., None))));
    }
}

#[test]
fn an_idle_hard_disk_keeps_its_reading_and_is_asked_after_transfers() {
    let file = FileFixture::new();
    let mut d = sata_disk(&file, false);
    let asked = std::cell::Cell::new(0);
    sample_power(&mut d, &asked, Ok(counters(1, 100)), true);
    for _ in 0..3 {
        sample_power(&mut d, &asked, Ok(counters(1, 100)), true);
        assert_eq!((asked.get(), d.temperature), (1, Some((31., None))));
    }
    sample_power(&mut d, &asked, Ok(counters(2, 300)), true);
    assert_eq!((asked.get(), d.temperature), (2, Some((32., None))));
    // Without counters there is no telling: asked as before.
    sample_power(&mut d, &asked, Err(1), true);
    sample_power(&mut d, &asked, Err(1), true);
    assert_eq!(asked.get(), 4);

    // An SSD has no idle time to protect.
    let file = FileFixture::new();
    let mut d = sata_disk(&file, true);
    let asked = std::cell::Cell::new(0);
    for _ in 0..3 {
        sample_power(&mut d, &asked, Ok(counters(1, 100)), true);
    }
    assert_eq!(asked.get(), 3);
}

#[test]
#[ignore = "read-only power state of the physical disks; sends nothing to a drive"]
fn live_power_state() {
    use windows::Win32::System::Power::GetDevicePowerState;
    let tracker = DiskTracker::new();
    for disk in &tracker.disks {
        let Some(handle) = &disk.handle else { continue };
        let report = handle.read(|raw, _| {
            let mut on = windows::core::BOOL(0);
            let started = Instant::now();
            let rounds = 1000;
            let mut answered = false;
            for _ in 0..rounds {
                answered = unsafe { GetDevicePowerState(raw, &mut on) }.as_bool();
            }
            Ok(
                serde_json::json!({"disk": disk.info.index, "bus": disk.info.bus,
                "answered": answered, "on": on.as_bool(), "working": working(raw),
                "queryMicros": started.elapsed().as_secs_f64() * 1e6 / f64::from(rounds)}),
            )
        });
        println!("{report:?}");
    }
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
#[ignore = "read-only comparison of physical and volume performance interfaces"]
fn live_performance_paths() {
    for (index, volumes) in volume_map() {
        for path in std::iter::once(format!(r"\\.\PhysicalDrive{index}"))
            .chain(volumes.into_iter().map(|letter| format!(r"\\.\{letter}")))
        {
            let result =
                DeviceHandle::open(&path).and_then(|handle| handle.read(|raw, _| performance(raw)));
            match result {
                Ok(value) => println!(
                    "{path}: manager={:?}; number={}; read={}; write={}; idle={}; time={}",
                    String::from_utf16_lossy(&value.StorageManagerName),
                    value.StorageDeviceNumber,
                    value.BytesRead,
                    value.BytesWritten,
                    value.IdleTime,
                    value.QueryTime
                ),
                Err(code) => println!("{path}: error={code}"),
            }
        }
    }
    let query = crate::pdh::Query::open().unwrap();
    let counters = [
        r"\PhysicalDisk(*)\Disk Read Bytes/sec",
        r"\PhysicalDisk(*)\% Idle Time",
        r"\LogicalDisk(*)\Disk Read Bytes/sec",
    ]
    .map(|path| (path, query.add(PCWSTR(wide(path).as_ptr()))));
    query.collect();
    std::thread::sleep(Duration::from_millis(600));
    query.collect();
    let mut buffer = AlignedBuf::with_capacity(16384);
    for (path, counter) in counters {
        println!(
            "{path}: {:?}",
            counter.map(|c| crate::pdh::array(c, &mut buffer))
        );
    }
}

#[test]
#[ignore = "read-only volume compatibility probe with simulated physical counter rejection"]
fn live_volume_fallback() {
    let mut tracker = DiskTracker::new();
    let before = tracker.collect_with_fallback(|_| Err(1), volumes::read);
    std::thread::sleep(Duration::from_millis(600));
    let start = Instant::now();
    let after = tracker.collect_with_fallback(|_| Err(1), volumes::read);
    println!(
        "compatibility sample: {:?}; {}",
        start.elapsed(),
        serde_json::to_string(&after).unwrap()
    );
    assert_eq!(before.len(), after.len());
    assert!(
        after
            .iter()
            .any(|disk| disk.read.is_some() && disk.performance_volumes.is_some())
    );
    for disk in &tracker.disks {
        for volume in &disk.info.volumes {
            disk.handle
                .as_ref()
                .unwrap()
                .read(|_, _| {
                    assert!(volumes::read(disk.info.index + 100, volume).is_err());
                    Ok(())
                })
                .unwrap();
        }
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
