//! One in-flight native query with a bounded wait on the sampling thread.
//! A late reply is recycled, never published with a new sample's timestamp.

use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError};
use std::time::Duration;

use windows::Wdk::System::SystemInformation::SystemProcessInformation;
use windows::Win32::Foundation::NTSTATUS;

use crate::diagnostics;
use crate::nt::query_system_information;
use crate::util::AlignedBuf;

const WAIT_BUDGET: Duration = Duration::from_millis(50);

struct Reply {
    buffer: AlignedBuf,
    result: Result<usize, NTSTATUS>,
}

pub(super) struct ProcessQuery {
    request: SyncSender<AlignedBuf>,
    response: Receiver<Reply>,
    buffer: Option<AlignedBuf>,
    pending: bool,
}

impl ProcessQuery {
    pub fn new() -> Option<Self> {
        Self::spawn(|buffer| {
            diagnostics::sample_phase("process-query", || {
                query_system_information(SystemProcessInformation, buffer)
            })
        })
        .inspect_err(|error| {
            diagnostics::warn("process query worker", diagnostics::io(error));
        })
        .ok()
    }

    fn spawn(
        mut query: impl FnMut(&mut AlignedBuf) -> Result<usize, NTSTATUS> + Send + 'static,
    ) -> std::io::Result<Self> {
        let (request, requests) = mpsc::sync_channel::<AlignedBuf>(1);
        let (replies, response) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("argus-process-query".into())
            .spawn(move || {
                while let Ok(mut buffer) = requests.recv() {
                    let result = query(&mut buffer);
                    if replies.send(Reply { buffer, result }).is_err() {
                        break;
                    }
                }
            })?;
        Ok(Self {
            request,
            response,
            buffer: Some(AlignedBuf::with_capacity(512 * 1024)),
            pending: false,
        })
    }

    pub fn read(&mut self) -> Option<(&AlignedBuf, usize)> {
        if self.pending {
            match self.response.try_recv() {
                Ok(reply) => {
                    // This request exceeded the preceding sample's budget. Its
                    // counters may span an unbounded interval inside the kernel.
                    self.buffer = Some(reply.buffer);
                    self.pending = false;
                }
                Err(TryRecvError::Empty) => return None,
                Err(TryRecvError::Disconnected) => {
                    diagnostics::warn("process query worker", "worker disconnected");
                    return None;
                }
            }
        }
        let buffer = self.buffer.take()?;
        if let Err(error) = self.request.try_send(buffer) {
            self.buffer = Some(match error {
                mpsc::TrySendError::Full(buffer) | mpsc::TrySendError::Disconnected(buffer) => {
                    buffer
                }
            });
            diagnostics::warn("process query worker", "request unavailable");
            return None;
        }
        self.pending = true;
        match self.response.recv_timeout(WAIT_BUDGET) {
            Ok(reply) => {
                self.pending = false;
                self.buffer = Some(reply.buffer);
                match reply.result {
                    Ok(returned) => Some((self.buffer.as_ref()?, returned)),
                    Err(status) => {
                        diagnostics::warn("process query", diagnostics::code(status.0 as u32));
                        None
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                diagnostics::warn("process query", "exceeded 50 ms; query continues on worker");
                None
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                diagnostics::warn("process query worker", "worker disconnected");
                None
            }
        }
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
    fn blocked_query_has_one_worker_and_late_data_is_discarded() {
        let calls = Arc::new(AtomicUsize::new(0));
        let count = calls.clone();
        let (release, gate) = mpsc::channel();
        let mut query = ProcessQuery::spawn(move |_| {
            let call = count.fetch_add(1, Ordering::SeqCst) + 1;
            if call == 1 {
                gate.recv_timeout(Duration::from_secs(3)).unwrap();
            }
            Ok(call)
        })
        .unwrap();
        let started = Instant::now();
        assert!(query.read().is_none());
        assert!(started.elapsed() < Duration::from_secs(1));
        let started = Instant::now();
        for _ in 0..20 {
            assert!(query.read().is_none());
        }
        assert!(started.elapsed() < Duration::from_millis(100));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        release.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if let Some((_, returned)) = query.read() {
                assert_eq!(returned, 2);
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn failed_query_reuses_its_buffer_and_recovers() {
        let mut first = true;
        let mut query = ProcessQuery::spawn(move |buffer| {
            if std::mem::take(&mut first) {
                buffer.grow_to(1024 * 1024);
                Err(NTSTATUS(0xc000_0001u32 as i32))
            } else {
                Ok(buffer.len())
            }
        })
        .unwrap();
        assert!(query.read().is_none());
        assert_eq!(query.read().unwrap().1, 1024 * 1024);
    }

    #[test]
    fn dropping_a_blocked_query_does_not_join_the_worker() {
        let (release, gate) = mpsc::channel();
        let (finished, done) = mpsc::channel();
        let mut query = ProcessQuery::spawn(move |_| {
            gate.recv_timeout(Duration::from_secs(3)).unwrap();
            finished.send(()).unwrap();
            Ok(0)
        })
        .unwrap();
        assert!(query.read().is_none());
        let started = Instant::now();
        drop(query);
        assert!(started.elapsed() < Duration::from_millis(100));
        release.send(()).unwrap();
        done.recv_timeout(Duration::from_secs(2)).unwrap();
    }
}
