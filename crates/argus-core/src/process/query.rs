//! Bounded native queries. Slow successful replies keep their original clock
//! readings. A second buffer lets a new query run while the last reply is consumed.
use crate::cpu::{CycleStamp, SystemCycles};
use crate::diagnostics;
use crate::nt::query_system_information;
use crate::util::{AlignedBuf, filetime_ticks};
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError};
use std::time::{Duration, Instant};
use windows::Wdk::System::SystemInformation::SystemProcessInformation;
use windows::Win32::Foundation::NTSTATUS;
use windows::Win32::System::SystemInformation::GetSystemTimePreciseAsFileTime;

const WAIT_BUDGET: Duration = Duration::from_millis(50);

pub(super) struct Observation {
    pub started: Instant,
    pub clock: CycleStamp,
    pub systime: i64,
    pub cycles: Option<SystemCycles>,
    pub logical_cpus: u32,
}
struct Reply {
    buffer: AlignedBuf,
    result: Result<usize, NTSTATUS>,
    observation: Observation,
}
pub(super) struct ProcessQuery {
    request: SyncSender<(AlignedBuf, u32)>,
    response: Receiver<Reply>,
    current: AlignedBuf,
    spare: Option<AlignedBuf>,
    pending: bool,
}
impl ProcessQuery {
    pub fn new() -> Option<Self> {
        Self::spawn(|buffer| {
            diagnostics::sample_phase("process-query", || {
                query_system_information(SystemProcessInformation, buffer)
            })
        })
        .inspect_err(|error| diagnostics::warn("process query worker", diagnostics::io(error)))
        .ok()
    }
    pub(super) fn spawn(
        mut query: impl FnMut(&mut AlignedBuf) -> Result<usize, NTSTATUS> + Send + 'static,
    ) -> std::io::Result<Self> {
        let (request, requests) = mpsc::sync_channel::<(AlignedBuf, u32)>(1);
        let (replies, response) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("argus-process-query".into())
            .spawn(move || {
                while let Ok((mut buffer, logical_cpus)) = requests.recv() {
                    let started = Instant::now();
                    let result = query(&mut buffer);
                    let observation = Observation {
                        started,
                        cycles: SystemCycles::read(logical_cpus),
                        clock: CycleStamp::capture(),
                        systime: filetime_ticks(unsafe { GetSystemTimePreciseAsFileTime() }),
                        logical_cpus,
                    };
                    #[cfg(debug_assertions)]
                    {
                        let delay = super::testing::take_delay();
                        if delay > 0 {
                            super::testing::active(true);
                            std::thread::sleep(Duration::from_millis(delay));
                            super::testing::active(false);
                        }
                    }
                    if replies
                        .send(Reply {
                            buffer,
                            result,
                            observation,
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            })?;
        Ok(Self {
            request,
            response,
            current: AlignedBuf::with_capacity(512 * 1024),
            spare: Some(AlignedBuf::with_capacity(512 * 1024)),
            pending: false,
        })
    }
    fn accept(&mut self, reply: Reply) -> Option<(usize, Observation)> {
        self.pending = false;
        match reply.result {
            Ok(returned) => {
                self.spare = Some(std::mem::replace(&mut self.current, reply.buffer));
                Some((returned, reply.observation))
            }
            Err(status) => {
                self.spare = Some(reply.buffer);
                diagnostics::warn("process query", diagnostics::code(status.0 as u32));
                None
            }
        }
    }
    pub fn read(&mut self, logical_cpus: u32) -> Option<(&AlignedBuf, usize, Observation)> {
        let mut latest = None;
        if self.pending {
            match self.response.try_recv() {
                Ok(reply) => latest = self.accept(reply),
                Err(TryRecvError::Empty) => return None,
                Err(TryRecvError::Disconnected) => {
                    diagnostics::warn("process query worker", "worker disconnected");
                    return None;
                }
            }
        }
        if let Some(buffer) = self.spare.take() {
            match self.request.try_send((buffer, logical_cpus)) {
                Ok(()) => {
                    self.pending = true;
                    match self.response.recv_timeout(WAIT_BUDGET) {
                        Ok(reply) => {
                            if let Some(fresh) = self.accept(reply) {
                                latest = Some(fresh);
                            }
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => diagnostics::warn(
                            "process query",
                            "exceeded 50 ms; retaining reply for next poll",
                        ),
                        Err(mpsc::RecvTimeoutError::Disconnected) => {
                            diagnostics::warn("process query worker", "worker disconnected")
                        }
                    }
                }
                Err(error) => {
                    let (buffer, _) = match error {
                        mpsc::TrySendError::Full(request)
                        | mpsc::TrySendError::Disconnected(request) => request,
                    };
                    self.spare = Some(buffer);
                }
            }
        }
        latest.map(|(returned, observed)| (&self.current, returned, observed))
    }
}

#[cfg(test)]
mod tests {
    use super::ProcessQuery;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    };
    use std::time::{Duration, Instant};
    use windows::Win32::Foundation::NTSTATUS;

    #[test]
    fn successful_slow_reads_are_delivered() {
        let mut call = 0;
        let mut query = ProcessQuery::spawn(move |_| {
            std::thread::sleep(Duration::from_millis(100));
            call += 1;
            Ok(call)
        })
        .unwrap();
        assert!(query.read(1).is_none());
        for expected in 1..=4 {
            std::thread::sleep(Duration::from_millis(120));
            let before = Instant::now();
            let (_, returned, observed) = query.read(1).expect("completed slow query");
            assert_eq!(returned, expected);
            assert!(observed.clock.at < before);
            assert!(before.elapsed() < Duration::from_millis(250));
        }
    }
    #[test]
    fn blocked_query_stays_bounded_and_completed_buffer_is_not_overwritten() {
        let calls = Arc::new(AtomicUsize::new(0));
        let count = calls.clone();
        let (release, gate) = mpsc::channel();
        let mut query = ProcessQuery::spawn(move |buffer| {
            let call = count.fetch_add(1, Ordering::SeqCst) + 1;
            unsafe {
                *buffer.as_mut_ptr().cast::<usize>() = call;
            }
            let _ = gate.recv_timeout(Duration::from_secs(3));
            Ok(call)
        })
        .unwrap();
        assert!(query.read(1).is_none());
        let started = Instant::now();
        for _ in 0..20 {
            assert!(query.read(1).is_none());
        }
        assert!(started.elapsed() < Duration::from_millis(100));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        release.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if let Some((buffer, returned, _)) = query.read(1) {
                assert_eq!(returned, 1);
                assert_eq!(unsafe { *buffer.as_ptr().cast::<usize>() }, 1);
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        let started = Instant::now();
        drop(query);
        assert!(started.elapsed() < Duration::from_millis(100));
        release.send(()).unwrap();
    }
    #[test]
    fn failed_refresh_keeps_the_completed_slow_reply() {
        let mut first = true;
        let mut query = ProcessQuery::spawn(move |_| {
            if std::mem::take(&mut first) {
                std::thread::sleep(Duration::from_millis(100));
                Ok(42)
            } else {
                Err(NTSTATUS(0xc000_0001u32 as i32))
            }
        })
        .unwrap();
        assert!(query.read(1).is_none());
        std::thread::sleep(Duration::from_millis(120));
        assert_eq!(query.read(1).map(|(_, returned, _)| returned), Some(42));
    }
}
