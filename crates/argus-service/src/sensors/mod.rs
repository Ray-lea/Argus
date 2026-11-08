//! Leased sensor reader in the SYSTEM service, and a nonblocking GUI client.
//! IPC accepts only a snapshot request, never an address, module, or write.

mod cpu;
mod poll;

use crate::{Operation, Reply};
use argus_core::hardware::{CpuSensors, SensorState};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

const POLICY: crate::snapshot::Policy = crate::snapshot::Policy {
    period: Duration::from_millis(500),
    max_age: Duration::from_secs(3),
    idle: Duration::from_secs(15),
};

pub(crate) struct SensorMonitor {
    reader: crate::snapshot::Reader<CpuSensors>,
    package: Option<crate::snapshot::Reader<CpuSensors>>,
}

impl SensorMonitor {
    pub fn spawn() -> Self {
        Self {
            reader: crate::snapshot::Reader::spawn(
                "argus-service-sensors",
                POLICY,
                NativeSource::default,
            ),
            package: cpu::CpuReader::supports_package_reader().then(|| {
                crate::snapshot::Reader::spawn("argus-service-package", POLICY, || NativeSource {
                    package_only: true,
                    ..Default::default()
                })
            }),
        }
    }

    #[cfg(test)]
    pub fn disabled() -> Self {
        Self {
            reader: crate::snapshot::Reader::disabled(POLICY),
            package: None,
        }
    }

    pub fn snapshot(&self) -> CpuSensors {
        if let Some(package) = &self.package {
            let detail = self.reader.snapshot_without_wait();
            return combine_package(package.snapshot(), detail);
        }
        self.reader
            .snapshot()
            .map(|frame| (*frame).clone())
            .unwrap_or_else(|| CpuSensors::unavailable(SensorState::Pending))
    }

    #[cfg(test)]
    pub(crate) fn with_reader(read: impl FnMut() -> CpuSensors + Send + 'static) -> Self {
        Self {
            reader: crate::snapshot::Reader::spawn(
                "sensor-test",
                crate::snapshot::tests::POLICY,
                move || crate::snapshot::tests::Callback(read),
            ),
            package: None,
        }
    }
}

/// The dedicated Intel reader owns these fields; delayed per-core data must not
/// replace current package readings or resurrect an unavailable package metric.
fn combine_package(
    package: Option<Arc<CpuSensors>>,
    detail: Option<Arc<CpuSensors>>,
) -> CpuSensors {
    use argus_core::hardware::CpuMetricKind as Kind;
    let Some(package) = package else {
        return detail.map(|frame| (*frame).clone()).unwrap_or_default();
    };
    if package.state != SensorState::Ready {
        return detail
            .filter(|frame| frame.state == SensorState::Ready)
            .map(|frame| (*frame).clone())
            .unwrap_or_else(|| (*package).clone());
    }
    let mut frame = detail
        .filter(|frame| frame.state == SensorState::Ready)
        .map(|frame| (*frame).clone())
        .unwrap_or_default();
    frame.state = SensorState::Ready;
    frame.package_celsius = package.package_celsius;
    frame.package_watts = package.package_watts;
    frame.limits = package.limits.clone();
    if frame.core_temperatures.is_empty() {
        frame.core_temperatures = package.core_temperatures.clone();
    }
    frame.metrics.retain(|metric| {
        !matches!(
            metric.kind,
            Kind::BusMhz
                | Kind::ThermalLimitCelsius
                | Kind::Pl1Watts
                | Kind::Pl2Watts
                | Kind::Pl1Seconds
                | Kind::Pl2Seconds
                | Kind::TdpWatts
                | Kind::RingMhz
                | Kind::RingMinMhz
                | Kind::RingMaxMhz
                | Kind::RingMultiplier
                | Kind::CoresWatts
                | Kind::GraphicsWatts
                | Kind::MemoryWatts
                | Kind::PlatformWatts
        )
    });
    frame.metrics.extend(package.metrics.iter().cloned());
    frame
}

#[derive(Default)]
struct NativeSource {
    reader: Option<cpu::CpuReader>,
    attempt: Option<Instant>,
    driver_version: Option<u32>,
    state: SensorState,
    package_only: bool,
    affinity: Option<cpu::Affinity>,
}

impl crate::snapshot::Source for NativeSource {
    type Value = CpuSensors;
    fn read(&mut self) -> CpuSensors {
        if self.package_only && self.affinity.is_none() {
            self.affinity = cpu::Affinity::set(0, 0);
            if self.affinity.is_none() {
                return CpuSensors::unavailable(SensorState::NoData);
            }
        }
        let version = if self.reader.is_none() {
            crate::driver::device_version()
        } else {
            self.driver_version
        };
        if self.reader.is_none()
            && (version != self.driver_version
                || self
                    .attempt
                    .is_none_or(|at| at.elapsed() >= Duration::from_secs(30)))
        {
            self.driver_version = version;
            self.attempt = Some(Instant::now());
            let opened = if self.package_only {
                cpu::CpuReader::open_package()
            } else {
                cpu::CpuReader::open()
            };
            match opened {
                Ok(reader) => self.reader = Some(reader),
                Err(state) => self.state = state,
            }
        }
        self.reader
            .as_mut()
            .map(|reader| {
                if self.package_only {
                    reader.sample_package()
                } else {
                    reader.sample()
                }
            })
            .unwrap_or_else(|| CpuSensors::unavailable(self.state))
    }
    fn idle(&mut self) {
        self.reader = None;
        self.attempt = None;
        self.state = SensorState::Pending;
        self.affinity = None;
    }
}

type CachedFrame = Option<(Instant, Arc<CpuSensors>)>;
type Cache = Arc<Mutex<CachedFrame>>;
pub struct SensorClient {
    tx: mpsc::Sender<Option<Duration>>,
    cache: Cache,
    last_cache: CachedFrame,
    last: Option<Instant>,
}

impl SensorClient {
    pub fn spawn() -> Self {
        let mut diagnostics = crate::diagnostics::Probe::default();
        Self::with_reader(move || {
            let pid = crate::util::stage("sensor service lookup", crate::install::running_pid());
            let Ok(Some(pid)) = pid else {
                argus_core::diagnostics::warn(
                    "kernel sensors unavailable",
                    "no running, authorized service",
                );
                return CpuSensors::unavailable(SensorState::ServiceUnavailable);
            };
            let reply = crate::util::stage(
                "sensor snapshot request",
                crate::pipe::request_with_timeout(
                    crate::PIPE_NAME,
                    pid,
                    Operation::SensorSnapshot {},
                    Duration::from_secs(2),
                ),
            );
            match reply {
                Ok(Reply::Sensors(frame)) => {
                    if !matches!(frame.state, SensorState::Ready | SensorState::Pending)
                        || frame.integrated_gpu.as_ref().is_some_and(|g| {
                            !matches!(g.state, SensorState::Ready | SensorState::Pending)
                                || (g.state == SensorState::Ready
                                    && (g.temperature_celsius.is_none()
                                        || g.clock_mhz.is_none()
                                        || g.vid_volts.is_none()
                                        || g.memory_clock_mhz.is_none()
                                        || g.limits.is_empty()))
                        })
                    {
                        argus_core::diagnostics::warn(
                            "kernel sensors unavailable",
                            format!(
                                "state={:?}; integrated_gpu={:?}",
                                frame.state,
                                frame.integrated_gpu.as_ref().map(|g| g.state)
                            ),
                        );
                        diagnostics.collect(pid);
                    }
                    *frame
                }
                other => {
                    if other.is_ok() {
                        argus_core::diagnostics::warn(
                            "sensor snapshot request",
                            "unexpected reply type",
                        );
                    }
                    CpuSensors::unavailable(SensorState::ServiceUnavailable)
                }
            }
        })
    }

    fn with_reader(mut read: impl FnMut() -> CpuSensors + Send + 'static) -> Self {
        let (tx, rx) = mpsc::channel();
        let cache: Cache = Arc::default();
        let output = cache.clone();
        let _ = std::thread::Builder::new()
            .name("argus-sensor-client".into())
            .spawn(move || {
                poll::run(rx, || {
                    let frame = Arc::new(read());
                    if let Ok(mut cache) = output.lock() {
                        *cache = Some((Instant::now(), frame));
                    }
                });
            })
            .inspect_err(|error| {
                argus_core::diagnostics::warn(
                    "sensor client worker creation",
                    argus_core::diagnostics::io(error),
                )
            });
        Self {
            tx,
            cache,
            last_cache: None,
            last: None,
        }
    }

    pub fn disable(&mut self) {
        if self.last.take().is_some() {
            let _ = self.tx.send(None);
        }
        self.last_cache = None;
    }

    pub fn enrich(
        &mut self,
        snapshot: &mut argus_core::Snapshot,
        enabled: bool,
        interval: Duration,
    ) {
        if !enabled {
            self.disable();
            snapshot.hardware.cpu_sensors = None;
            self.last_cache = None;
            return;
        }
        if self
            .last
            .is_none_or(|at| at.elapsed() >= Duration::from_millis(500))
            && self
                .tx
                .send(Some(interval.max(Duration::from_millis(500))))
                .is_ok()
        {
            self.last = Some(Instant::now());
        }
        if let Ok(cache) = self.cache.try_lock() {
            self.last_cache = cache.clone();
        }
        snapshot.hardware.cpu_sensors = Some(
            self.last_cache
                .as_ref()
                .filter(|(at, _)| at.elapsed() < interval.max(Duration::from_secs(1)) * 3)
                .map(|(_, frame)| (**frame).clone())
                .unwrap_or_default(),
        );
        if snapshot.hardware.power.package.is_none() {
            snapshot.hardware.power.package = snapshot
                .hardware
                .cpu_sensors
                .as_ref()
                .and_then(|s| s.package_watts);
        }
        if let Some(frame) = &mut snapshot.hardware.cpu_sensors {
            use argus_core::hardware::CpuMetricKind;
            for (target, kind) in [
                (
                    &mut snapshot.hardware.power.cores,
                    CpuMetricKind::CoresWatts,
                ),
                (
                    &mut snapshot.hardware.power.graphics,
                    CpuMetricKind::GraphicsWatts,
                ),
                (
                    &mut snapshot.hardware.power.memory,
                    CpuMetricKind::MemoryWatts,
                ),
            ] {
                if target.is_none() {
                    *target = frame.value(kind);
                }
            }
            for core in &mut frame.cores {
                let samples: Vec<_> = snapshot
                    .hardware
                    .cpu
                    .threads
                    .iter()
                    .enumerate()
                    .filter(|(_, thread)| thread.core == core.core)
                    .map(|(index, _)| snapshot.hardware.cpu_idle.get(index))
                    .collect();
                let average = |field: fn(&argus_core::hardware::CpuIdle) -> Option<f32>| {
                    let values: Option<Vec<_>> = samples
                        .iter()
                        .map(|sample| sample.and_then(field))
                        .collect();
                    values
                        .filter(|v| !v.is_empty())
                        .map(|v| v.iter().sum::<f32>() / v.len() as f32)
                };
                core.os_c1_percent = average(|s| s.c1);
                core.os_c2_percent = average(|s| s.c2);
                core.os_c3_percent = average(|s| s.c3);
                if core.clock_mhz.is_none() {
                    core.clock_mhz = snapshot
                        .hardware
                        .cpu
                        .thread_index(core.group, core.processor)
                        .and_then(|i| snapshot.hardware.core_mhz.get(i))
                        .copied()
                        .filter(|mhz| *mhz > 0)
                        .map(|mhz| mhz as f32);
                }
            }
        }
        snapshot.hardware.supplement_integrated_gpu();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slow_per_core_reads_do_not_delay_fresh_package_data() {
        use std::sync::atomic::{AtomicU32, Ordering};
        let (entered, started) = mpsc::channel();
        let (release, gate) = mpsc::channel();
        let mut monitor = SensorMonitor::with_reader(move || {
            let _ = entered.send(());
            let _ = gate.recv_timeout(Duration::from_secs(5));
            CpuSensors::unavailable(SensorState::NoData)
        });
        let reads = Arc::new(AtomicU32::new(0));
        let count = reads.clone();
        monitor.package = Some(crate::snapshot::Reader::spawn(
            "package-test",
            crate::snapshot::tests::POLICY,
            move || {
                crate::snapshot::tests::Callback(move || CpuSensors {
                    state: SensorState::Ready,
                    package_celsius: Some(64. + count.fetch_add(1, Ordering::Relaxed) as f32),
                    package_watts: Some(30.),
                    ..Default::default()
                })
            },
        ));
        let first = monitor.snapshot();
        started.recv_timeout(Duration::from_secs(2)).unwrap();
        let mut elapsed = Duration::ZERO;
        let mut last = first.package_celsius;
        for _ in 0..10 {
            let at = Instant::now();
            let frame = monitor.snapshot();
            elapsed = elapsed.max(at.elapsed());
            assert_eq!(frame.state, SensorState::Ready);
            last = frame.package_celsius;
            std::thread::sleep(crate::snapshot::tests::POLICY.period * 2);
        }
        release.send(()).unwrap();
        monitor.reader.stop_and_join();
        monitor.package.as_mut().unwrap().stop_and_join();
        assert_eq!(first.package_watts, Some(30.));
        assert!(last > first.package_celsius);
        assert!(reads.load(Ordering::Relaxed) > 1);
        assert!(elapsed < Duration::from_millis(100));
    }

    #[test]
    fn package_merge_preserves_core_data_and_replaces_package_metrics() {
        use argus_core::hardware::{CpuCoreSensors, CpuMetricKind as Kind, CpuSensorSource};
        let mut detail = CpuSensors {
            state: SensorState::Ready,
            package_celsius: Some(40.),
            cores: vec![CpuCoreSensors {
                celsius: Some(63.),
                ..Default::default()
            }],
            ..Default::default()
        };
        detail.metric(Kind::Pl1Watts, Some(100.), CpuSensorSource::Msr);
        detail.metric(Kind::AverageMhz, Some(3600.), CpuSensorSource::Msr);
        let mut package = CpuSensors {
            state: SensorState::Ready,
            package_celsius: Some(64.),
            ..Default::default()
        };
        package.metric(Kind::Pl1Watts, Some(80.), CpuSensorSource::Msr);
        let frame = combine_package(Some(Arc::new(package)), Some(Arc::new(detail)));
        assert_eq!(frame.package_celsius, Some(64.));
        assert_eq!(frame.cores[0].celsius, Some(63.));
        assert_eq!(frame.value(Kind::Pl1Watts), Some(80.));
        assert_eq!(
            frame
                .metrics
                .iter()
                .filter(|m| m.kind == Kind::Pl1Watts)
                .count(),
            1
        );
        assert_eq!(frame.value(Kind::AverageMhz), Some(3600.));
        let failure = combine_package(
            Some(Arc::new(CpuSensors::unavailable(
                SensorState::DriverUnavailable,
            ))),
            None,
        );
        assert_eq!(failure.state, SensorState::DriverUnavailable);
        assert!(failure.package_celsius.is_none());
    }

    #[test]
    fn a_concurrent_cache_write_keeps_the_recent_sensor_frame_without_blocking() {
        let mut client = SensorClient::with_reader(|| CpuSensors::default());
        client.last = Some(Instant::now());
        *client.cache.lock().unwrap() = Some((
            Instant::now(),
            Arc::new(CpuSensors {
                state: SensorState::Ready,
                package_celsius: Some(68.),
                ..Default::default()
            }),
        ));
        let mut snapshot = argus_core::Snapshot {
            seq: 0,
            timestamp: 0,
            system: Default::default(),
            hardware: Default::default(),
            processes: vec![],
            process_network: false,
        };
        client.enrich(&mut snapshot, true, Duration::from_secs(1));
        let shared = client.cache.clone();
        let guard = shared.lock().unwrap();
        let start = Instant::now();
        client.enrich(&mut snapshot, true, Duration::from_secs(1));
        assert!(start.elapsed() < Duration::from_millis(100));
        assert_eq!(
            snapshot
                .hardware
                .cpu_sensors
                .as_ref()
                .unwrap()
                .package_celsius,
            Some(68.)
        );
        client.last_cache.as_mut().unwrap().0 = Instant::now() - Duration::from_secs(4);
        client.enrich(&mut snapshot, true, Duration::from_secs(1));
        assert!(
            snapshot
                .hardware
                .cpu_sensors
                .as_ref()
                .unwrap()
                .package_celsius
                .is_none()
        );
        drop(guard);
        *shared.lock().unwrap() = Some((
            Instant::now(),
            Arc::new(CpuSensors::unavailable(SensorState::NoData)),
        ));
        client.enrich(&mut snapshot, true, Duration::from_secs(1));
        assert_eq!(
            snapshot.hardware.cpu_sensors.as_ref().unwrap().state,
            SensorState::NoData
        );
        assert!(
            snapshot
                .hardware
                .cpu_sensors
                .unwrap()
                .package_celsius
                .is_none()
        );
    }

    #[test]
    fn refreshing_sensors_keeps_the_recent_frame_without_waiting_for_hardware() {
        let (release, gate) = mpsc::channel();
        let (entered, started) = mpsc::channel();
        let mut count = 0;
        let mut monitor = SensorMonitor::with_reader(move || {
            count += 1;
            if count == 2 {
                entered.send(()).unwrap();
                let _ = gate.recv_timeout(Duration::from_secs(3));
            }
            CpuSensors {
                state: SensorState::Ready,
                package_celsius: Some(62.0),
                ..Default::default()
            }
        });
        assert_eq!(monitor.snapshot().state, SensorState::Ready);
        std::thread::sleep(crate::snapshot::tests::POLICY.period * 2);
        assert_eq!(monitor.snapshot().state, SensorState::Ready);
        started.recv_timeout(Duration::from_secs(2)).unwrap();
        let start = Instant::now();
        let frame = monitor.snapshot();
        let elapsed = start.elapsed();
        release.send(()).unwrap();
        monitor.reader.stop_and_join();
        println!(
            "refresh state={:?}; elapsed_ms={}",
            frame.state,
            elapsed.as_millis()
        );
        assert_eq!(frame.state, SensorState::Ready);
        assert!(elapsed < Duration::from_millis(100));
    }

    #[test]
    fn backend_sensor_failure_is_reported_instead_of_retaining_a_ready_frame() {
        let mut monitor =
            SensorMonitor::with_reader(|| CpuSensors::unavailable(SensorState::NoData));
        assert_eq!(monitor.snapshot().state, SensorState::NoData);
        monitor.reader.stop_and_join();
    }

    #[test]
    fn enrich_preserves_windows_power_and_keeps_zero_sensor_readings() {
        use argus_core::hardware::{CpuCoreSensors, CpuMetricKind, CpuSensorSource};
        let mut client =
            SensorClient::with_reader(|| CpuSensors::unavailable(SensorState::ServiceUnavailable));
        let mut frame = CpuSensors {
            state: SensorState::Ready,
            package_watts: Some(101.0),
            cores: vec![CpuCoreSensors::default()],
            ..Default::default()
        };
        frame.metric(CpuMetricKind::CoresWatts, Some(0.0), CpuSensorSource::Msr);
        frame.metric(CpuMetricKind::MemoryWatts, Some(2.5), CpuSensorSource::Msr);
        *client.cache.lock().unwrap() = Some((Instant::now(), Arc::new(frame)));
        client.last = Some(Instant::now());
        let mut snapshot = argus_core::Snapshot {
            seq: 0,
            timestamp: 0,
            system: Default::default(),
            hardware: Default::default(),
            processes: vec![],
            process_network: false,
        };
        snapshot.hardware.power.package = Some(99.0);
        snapshot.hardware.core_mhz = vec![4200];
        std::sync::Arc::make_mut(&mut snapshot.hardware.cpu).threads =
            vec![argus_core::hardware::CpuThread::default()];
        client.enrich(&mut snapshot, true, Duration::from_secs(1));
        assert_eq!(snapshot.hardware.power.package, Some(99.0));
        assert_eq!(snapshot.hardware.power.cores, Some(0.0));
        assert_eq!(snapshot.hardware.power.memory, Some(2.5));
        assert_eq!(
            snapshot.hardware.cpu_sensors.unwrap().cores[0].clock_mhz,
            Some(4200.0)
        );
    }
    #[test]
    fn disconnected_reader_returns_without_driver_access() {
        let started = Instant::now();
        assert_eq!(
            SensorMonitor::disabled().snapshot().state,
            SensorState::Pending
        );
        assert!(started.elapsed() < Duration::from_millis(100));
    }

    #[test]
    fn disabled_sensors_never_query_and_slow_service_does_not_block_sampling() {
        let (started, entered) = mpsc::channel();
        let (release, gate) = mpsc::channel();
        let mut client = SensorClient::with_reader(move || {
            let _ = started.send(());
            let _ = gate.recv_timeout(Duration::from_secs(5));
            CpuSensors::unavailable(SensorState::DriverUnavailable)
        });
        let mut snapshot = argus_core::Snapshot {
            seq: 0,
            timestamp: 0,
            system: Default::default(),
            hardware: Default::default(),
            processes: vec![],
            process_network: false,
        };
        client.enrich(&mut snapshot, false, Duration::from_secs(1));
        assert!(snapshot.hardware.cpu_sensors.is_none());
        assert!(entered.try_recv().is_err());
        client.enrich(&mut snapshot, true, Duration::from_secs(1));
        entered.recv_timeout(Duration::from_secs(5)).unwrap();
        *client.cache.lock().unwrap() = Some((
            Instant::now() - Duration::from_secs(20),
            Arc::new(CpuSensors {
                state: SensorState::Ready,
                package_celsius: Some(77.0),
                ..Default::default()
            }),
        ));
        let start = Instant::now();
        for _ in 0..100 {
            client.enrich(&mut snapshot, true, Duration::from_secs(1));
        }
        assert!(start.elapsed() < Duration::from_millis(100));
        assert!(
            snapshot
                .hardware
                .cpu_sensors
                .unwrap()
                .package_celsius
                .is_none()
        );
        release.send(()).unwrap();
    }
}
