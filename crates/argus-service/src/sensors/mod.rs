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
        Self::with_reader(|| {
            let reply = crate::install::running_pid()
                .ok()
                .flatten()
                .and_then(|pid| {
                    crate::pipe::request_with_timeout(
                        crate::PIPE_NAME,
                        pid,
                        Operation::SensorSnapshot {},
                        Duration::from_secs(2),
                    )
                    .ok()
                });
            match reply {
                Some(Reply::Sensors(frame)) => frame,
                _ => CpuSensors::unavailable(SensorState::ServiceUnavailable),
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
