//! Intel logical processors are sampled by stationary, leased workers. A slow
//! migration during one worker's first read cannot starve the remaining cores.
use super::{Affinity, Identity, Module, telemetry::Telemetry};
use argus_core::hardware::{
    CoreTemperature, CpuCoreSensors, CpuInfo, CpuMetricKind as Kind, CpuSensorSource, CpuSensors,
    CpuThread,
};
use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{Duration, Instant},
};

pub(super) struct Readers {
    workers: Vec<(CpuThread, crate::snapshot::Reader<Option<CpuCoreSensors>>)>,
}

impl Readers {
    pub fn new(info: Arc<CpuInfo>, id: Identity, processors: &[(u16, u8)]) -> Self {
        let workers = processors
            .iter()
            .filter_map(|&(group, processor)| {
                let thread = *info.threads.get(info.thread_index(group, processor)?)?;
                let info = info.clone();
                Some((
                    thread,
                    crate::snapshot::Reader::spawn(
                        "argus-service-cpu-thread",
                        super::super::POLICY,
                        move || Source {
                            info,
                            id,
                            thread,
                            reader: None,
                            affinity: None,
                            attempt: None,
                        },
                    ),
                ))
            })
            .collect();
        Self { workers }
    }

    pub fn sample(&self, frame: &mut CpuSensors) {
        let readings = self.workers.iter().map(|(thread, worker)| {
            (
                *thread,
                worker
                    .snapshot_without_wait()
                    .and_then(|value| (*value).clone()),
            )
        });
        frame.cores = combine(readings);
        frame.core_temperatures = frame
            .cores
            .iter()
            .filter_map(|core| {
                Some(CoreTemperature {
                    group: core.group,
                    processor: core.processor,
                    celsius: core.celsius?,
                })
            })
            .collect();
        let mean = |field: fn(&CpuCoreSensors) -> Option<f32>| {
            let values: Option<Vec<_>> = frame.cores.iter().map(field).collect();
            values
                .filter(|values| !values.is_empty())
                .map(|values| values.iter().sum::<f32>() / values.len() as f32)
        };
        let average = mean(|core| core.clock_mhz);
        let effective = mean(|core| core.effective_mhz);
        frame.metric(Kind::AverageMhz, average, CpuSensorSource::Msr);
        frame.metric(Kind::EffectiveMhz, effective, CpuSensorSource::Msr);
        frame.core_vid = frame
            .cores
            .iter()
            .filter_map(|core| core.vid)
            .reduce(f32::max)
            .or(frame.core_vid);
        let flags: Option<Vec<_>> = frame
            .cores
            .iter()
            .map(|core| core.thermal_throttling)
            .collect();
        frame.thermal_throttling = flags
            .filter(|v| !v.is_empty())
            .map(|v| v.into_iter().any(|v| v));
    }
}

struct Source {
    info: Arc<CpuInfo>,
    id: Identity,
    thread: CpuThread,
    reader: Option<(Module, Telemetry)>,
    affinity: Option<Affinity>,
    attempt: Option<Instant>,
}

impl crate::snapshot::Source for Source {
    type Value = Option<CpuCoreSensors>;
    fn read(&mut self) -> Self::Value {
        if self.reader.is_none() {
            if self
                .attempt
                .is_some_and(|at| at.elapsed() < Duration::from_secs(30))
            {
                return None;
            }
            self.attempt = Some(Instant::now());
            if self.affinity.is_none() {
                self.affinity = Some(Affinity::set(self.thread.group, self.thread.processor)?);
            }
            let module = Module::open(super::INTEL).ok()?;
            let telemetry = Telemetry::new(
                &module,
                self.id,
                &self.info,
                &[(self.thread.group, self.thread.processor)],
            );
            self.reader = Some((module, telemetry));
        }
        let (module, telemetry) = self.reader.as_mut()?;
        let mut frame = CpuSensors::default();
        telemetry.sample_cores(module, self.id, &mut frame, Instant::now(), None);
        frame.cores.into_iter().next()
    }
    fn idle(&mut self) {
        self.reader = None;
        self.affinity = None;
        self.attempt = None;
    }
}

fn combine(
    readings: impl Iterator<Item = (CpuThread, Option<CpuCoreSensors>)>,
) -> Vec<CpuCoreSensors> {
    let mut groups = BTreeMap::<u32, Vec<_>>::new();
    for (thread, reading) in readings {
        groups
            .entry(thread.core)
            .or_default()
            .push((thread, reading));
    }
    groups
        .into_values()
        .map(|members| {
            let first = members[0].0;
            let mut core = members
                .iter()
                .find_map(|(_, value)| value.clone())
                .unwrap_or_default();
            core.core = first.core;
            core.group = first.group;
            core.processor = first.processor;
            core.efficient = first.efficient;
            let mean = |field: fn(&CpuCoreSensors) -> Option<f32>| {
                let values: Option<Vec<_>> = members
                    .iter()
                    .map(|(_, value)| value.as_ref().and_then(field))
                    .collect();
                values.map(|values| values.iter().sum::<f32>() / values.len() as f32)
            };
            core.effective_mhz = mean(|core| core.effective_mhz);
            core.c0_percent = mean(|core| core.c0_percent);
            core
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::combine;
    use argus_core::hardware::{CpuCoreSensors, CpuThread};
    #[test]
    fn slow_logical_processors_keep_identity_and_do_not_fabricate_smt_averages() {
        let thread = |core, processor| CpuThread {
            core,
            processor,
            ..Default::default()
        };
        let reading = |mhz| {
            Some(CpuCoreSensors {
                celsius: Some(65.),
                effective_mhz: Some(mhz),
                c0_percent: Some(50.),
                ..Default::default()
            })
        };
        let merged = combine(
            [
                (thread(0, 0), reading(1000.)),
                (thread(0, 1), reading(3000.)),
                (thread(1, 2), None),
                (thread(1, 3), reading(2000.)),
            ]
            .into_iter(),
        );
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].effective_mhz, Some(2000.));
        assert_eq!(merged[1].core, 1);
        assert_eq!(merged[1].processor, 2);
        assert_eq!(merged[1].celsius, Some(65.));
        assert_eq!(merged[1].effective_mhz, None);
        assert_eq!(merged[1].c0_percent, None);
    }
}
