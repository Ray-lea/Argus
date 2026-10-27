//! Leased sensor reader in the SYSTEM service, and a nonblocking GUI client.
//! IPC accepts only a snapshot request, never an address, module, or write.

mod cpu;

use crate::{Operation, Reply};
use argus_core::hardware::{CpuSensors, SensorState};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

type Request = mpsc::Sender<CpuSensors>;
pub(crate) struct SensorMonitor {
    tx: mpsc::SyncSender<Request>,
}

impl SensorMonitor {
    pub fn spawn() -> Self {
        let (tx, rx) = mpsc::sync_channel::<Request>(1);
        let _ = std::thread::Builder::new()
            .name("argus-service-sensors".into())
            .spawn(move || {
                let mut reader = None;
                let mut attempt: Option<Instant> = None;
                let mut last: Option<Instant> = None;
                let mut frame = CpuSensors::default();
                let mut driver_version = None;
                loop {
                    let request = match rx.recv_timeout(Duration::from_secs(15)) {
                        Ok(request) => request,
                        Err(mpsc::RecvTimeoutError::Timeout) => {
                            reader = None;
                            attempt = None;
                            last = None;
                            continue;
                        }
                        Err(mpsc::RecvTimeoutError::Disconnected) => return,
                    };
                    let current_version = if reader.is_none() {
                        crate::driver::device_version()
                    } else {
                        driver_version
                    };
                    if reader.is_none()
                        && (current_version != driver_version
                            || attempt.is_none_or(|at| at.elapsed() >= Duration::from_secs(30)))
                    {
                        driver_version = current_version;
                        attempt = Some(Instant::now());
                        match cpu::CpuReader::open() {
                            Ok(value) => reader = Some(value),
                            Err(state) => frame = CpuSensors::unavailable(state),
                        }
                    }
                    if let Some(reader) = &mut reader
                        && last.is_none_or(|at| at.elapsed() >= Duration::from_millis(500))
                    {
                        frame = reader.sample();
                        last = Some(Instant::now());
                    }
                    let _ = request.send(frame.clone());
                }
            })
            .inspect_err(|error| {
                argus_core::diagnostics::warn(
                    "sensor service worker creation",
                    argus_core::diagnostics::io(error),
                )
            });
        Self { tx }
    }

    #[cfg(test)]
    pub fn disabled() -> Self {
        let (tx, _) = mpsc::sync_channel(1);
        Self { tx }
    }

    pub fn snapshot(&self) -> CpuSensors {
        let (tx, rx) = mpsc::channel();
        if self.tx.try_send(tx).is_err() {
            return CpuSensors::unavailable(SensorState::Pending);
        }
        rx.recv_timeout(Duration::from_millis(800))
            .unwrap_or_else(|_| CpuSensors::unavailable(SensorState::NoData))
    }
}

type Cache = Arc<Mutex<Option<(Instant, CpuSensors)>>>;
pub struct SensorClient {
    tx: mpsc::SyncSender<()>,
    cache: Cache,
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
                                        || g.vid_volts.is_none()))
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
                    frame
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
        let (tx, rx) = mpsc::sync_channel(1);
        let cache: Cache = Arc::default();
        let output = cache.clone();
        let _ = std::thread::Builder::new()
            .name("argus-sensor-client".into())
            .spawn(move || {
                while rx.recv().is_ok() {
                    let frame = read();
                    if let Ok(mut cache) = output.lock() {
                        *cache = Some((Instant::now(), frame));
                    }
                }
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
            last: None,
        }
    }

    pub fn enrich(
        &mut self,
        snapshot: &mut argus_core::Snapshot,
        enabled: bool,
        interval: Duration,
    ) {
        if !enabled {
            snapshot.hardware.cpu_sensors = None;
            return;
        }
        if self
            .last
            .is_none_or(|at| at.elapsed() >= Duration::from_millis(500))
            && self.tx.try_send(()).is_ok()
        {
            self.last = Some(Instant::now());
        }
        snapshot.hardware.cpu_sensors = Some(
            self.cache
                .try_lock()
                .ok()
                .and_then(|cache| {
                    cache.as_ref().and_then(|(at, frame)| {
                        (at.elapsed() < interval.max(Duration::from_secs(1)) * 3)
                            .then(|| frame.clone())
                    })
                })
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
        *client.cache.lock().unwrap() = Some((Instant::now(), frame));
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
            CpuSensors {
                state: SensorState::Ready,
                package_celsius: Some(77.0),
                ..Default::default()
            },
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
