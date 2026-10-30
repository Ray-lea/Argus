//! Opt-in GPU timeline samples. Queries are recycled and read without flushing.
use std::{
    collections::VecDeque,
    mem::size_of,
    sync::{OnceLock, mpsc},
};
use windows::{
    Win32::{
        Graphics::Direct3D11::*,
        System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency},
    },
    core::{HRESULT, Interface},
};

const POOL_SIZE: usize = 8;
const SAMPLE_EVERY: u64 = 17;
const READ_DELAY: u64 = 4;

struct Queries {
    disjoint: ID3D11Query,
    marks: Vec<ID3D11Query>,
}
struct Sample {
    slot: usize,
    serial: u64,
    at_qpc: u64,
    details: String,
    present_cpu_us: Option<f64>,
}

pub(crate) struct GpuTimings<const N: usize> {
    context: ID3D11DeviceContext,
    kind: &'static str,
    queries: Vec<Queries>,
    free: Vec<usize>,
    pending: VecDeque<Sample>,
    active: Option<Sample>,
    written: usize,
    serial: u64,
    failed: bool,
}

fn query(device: &ID3D11Device, kind: D3D11_QUERY) -> windows::core::Result<ID3D11Query> {
    let mut result = None;
    unsafe {
        device.CreateQuery(
            &D3D11_QUERY_DESC {
                Query: kind,
                MiscFlags: 0,
            },
            Some(&mut result),
        )?;
    }
    result.ok_or_else(|| windows::core::Error::from_hresult(HRESULT(0x80004005u32 as i32)))
}

fn emit(record: String) {
    static SENDER: OnceLock<Option<mpsc::SyncSender<String>>> = OnceLock::new();
    if let Some(sender) = SENDER.get_or_init(|| {
        let (sender, receiver) = mpsc::sync_channel::<String>(32);
        std::thread::Builder::new()
            .name("argus-gpu-timing".into())
            .spawn(move || {
                while let Ok(record) = receiver.recv() {
                    println!("{record}");
                }
            })
            .ok()
            .map(|_| sender)
    }) {
        let _ = sender.try_send(record);
    }
}

fn qpc() -> u64 {
    static FREQUENCY: OnceLock<i64> = OnceLock::new();
    let frequency = *FREQUENCY.get_or_init(|| {
        let mut value = 0;
        unsafe {
            QueryPerformanceFrequency(&mut value).expect("QPC frequency");
        }
        value
    });
    let mut value = 0;
    unsafe {
        QueryPerformanceCounter(&mut value).expect("QPC counter");
    }
    (value as u128 * 10_000_000 / frequency as u128) as u64
}

// The generated Result<()> wrapper also accepts S_FALSE. Preserve HRESULT so
// a pending query never becomes an available zero-valued sample.
fn read<T: Default>(
    context: &ID3D11DeviceContext,
    query: &ID3D11Query,
) -> windows::core::Result<Option<T>> {
    let mut value = T::default();
    let status = unsafe {
        (context.vtable().GetData)(
            context.as_raw(),
            query.as_raw(),
            (&mut value as *mut T).cast(),
            size_of::<T>() as u32,
            D3D11_ASYNC_GETDATA_DONOTFLUSH.0 as u32,
        )
    };
    if status == HRESULT(0) {
        Ok(Some(value))
    } else {
        status.ok()?;
        Ok(None)
    }
}

fn durations(ticks: &[u64], frequency: u64, disjoint: bool) -> Option<Vec<f64>> {
    if disjoint
        || frequency == 0
        || ticks.len() < 2
        || ticks.windows(2).any(|pair| pair[1] < pair[0])
    {
        return None;
    }
    Some(
        ticks
            .windows(2)
            .map(|pair| (pair[1] - pair[0]) as f64 * 1_000_000. / frequency as f64)
            .collect(),
    )
}

impl<const N: usize> GpuTimings<N> {
    pub(crate) fn from_environment(
        device: &ID3D11Device,
        context: &ID3D11DeviceContext,
        kind: &'static str,
    ) -> Option<Self> {
        static ENABLED: OnceLock<bool> = OnceLock::new();
        if !*ENABLED.get_or_init(|| std::env::var_os("ARGUS_GPUI_GPU_TIMING").is_some()) {
            return None;
        }
        match Self::new(device, context, kind) {
            Ok(timing) => Some(timing),
            Err(error) => {
                emit(format!(
                    "{{\"event\":\"gpu-timing-error\",\"kind\":\"{kind}\",\"hresult\":{}}}",
                    error.code().0
                ));
                None
            }
        }
    }

    fn new(
        device: &ID3D11Device,
        context: &ID3D11DeviceContext,
        kind: &'static str,
    ) -> windows::core::Result<Self> {
        let mut queries = Vec::with_capacity(POOL_SIZE);
        for _ in 0..POOL_SIZE {
            let mut marks = Vec::with_capacity(N);
            for _ in 0..N {
                marks.push(query(device, D3D11_QUERY_TIMESTAMP)?);
            }
            queries.push(Queries {
                disjoint: query(device, D3D11_QUERY_TIMESTAMP_DISJOINT)?,
                marks,
            });
        }
        Ok(Self {
            context: context.clone(),
            kind,
            queries,
            free: (0..POOL_SIZE).collect(),
            pending: VecDeque::new(),
            active: None,
            written: 0,
            serial: 0,
            failed: false,
        })
    }

    pub(crate) fn begin(&mut self) {
        self.serial = self.serial.wrapping_add(1);
        if self.failed {
            return;
        }
        if let Err(error) = self.poll() {
            self.failed = true;
            emit(format!(
                "{{\"event\":\"gpu-timing-error\",\"kind\":\"{}\",\"hresult\":{}}}",
                self.kind,
                error.code().0
            ));
            return;
        }
        if !self.serial.is_multiple_of(SAMPLE_EVERY) {
            return;
        }
        let Some(slot) = self.free.pop() else {
            return;
        };
        unsafe {
            self.context.Begin(&self.queries[slot].disjoint);
            self.context.End(&self.queries[slot].marks[0]);
        }
        self.written = 1;
        self.active = Some(Sample {
            slot,
            serial: self.serial,
            at_qpc: qpc(),
            details: String::new(),
            present_cpu_us: None,
        });
    }

    pub(crate) fn mark(&mut self, index: usize) {
        if let Some(sample) = &self.active {
            while self.written <= index && self.written < N {
                unsafe {
                    self.context
                        .End(&self.queries[sample.slot].marks[self.written]);
                }
                self.written += 1;
            }
        }
    }

    pub(crate) fn finish(&mut self, details: impl FnOnce() -> String) {
        self.mark(N - 1);
        if let Some(mut sample) = self.active.take() {
            unsafe {
                self.context.End(&self.queries[sample.slot].disjoint);
            }
            sample.details = details();
            self.pending.push_back(sample);
        }
    }

    pub(crate) fn present(&mut self, cpu_us: f64) {
        if let Some(sample) = self.pending.back_mut()
            && sample.serial == self.serial
            && sample.present_cpu_us.is_none()
        {
            sample.present_cpu_us = Some(cpu_us);
        }
    }

    fn poll(&mut self) -> windows::core::Result<()> {
        while let Some(sample) = self.pending.front() {
            if self.serial.saturating_sub(sample.serial) < READ_DELAY {
                break;
            }
            let queries = &self.queries[sample.slot];
            let Some(status) =
                read::<D3D11_QUERY_DATA_TIMESTAMP_DISJOINT>(&self.context, &queries.disjoint)?
            else {
                break;
            };
            let mut ticks = Vec::with_capacity(N);
            for mark in &queries.marks {
                let Some(tick) = read::<u64>(&self.context, mark)? else {
                    return Ok(());
                };
                ticks.push(tick);
            }
            let sample = self.pending.pop_front().expect("pending sample");
            let times = durations(&ticks, status.Frequency, status.Disjoint.as_bool());
            let present = sample
                .present_cpu_us
                .map_or_else(|| "null".into(), |value| value.to_string());
            if let Some(times) = times {
                let total = times.iter().sum::<f64>();
                emit(format!(
                    "{{\"event\":\"gpu-timing\",\"kind\":\"{}\",\"frame\":{},\"atQpc\":{},\"valid\":true,\"timestampFrequencyHz\":{},\"gpuUs\":{times:?},\"totalGpuUs\":{total},\"presentCpuUs\":{present},{}}}",
                    self.kind, sample.serial, sample.at_qpc, status.Frequency, sample.details
                ));
            } else {
                emit(format!(
                    "{{\"event\":\"gpu-timing\",\"kind\":\"{}\",\"frame\":{},\"atQpc\":{},\"valid\":false,\"timestampFrequencyHz\":{},{}}}",
                    self.kind, sample.serial, sample.at_qpc, status.Frequency, sample.details
                ));
            }
            self.free.push(sample.slot);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::durations;
    #[test]
    fn timestamps_use_the_query_frequency_and_reject_invalid_intervals() {
        assert_eq!(
            durations(&[10, 12, 15], 1000, false),
            Some(vec![2000., 3000.])
        );
        assert!(durations(&[10, 12], 0, false).is_none());
        assert!(durations(&[10, 12], 1000, true).is_none());
        assert!(durations(&[12, 10], 1000, false).is_none());
        assert_eq!(durations(&[10, 10], 1000, false), Some(vec![0.]));
    }
}
