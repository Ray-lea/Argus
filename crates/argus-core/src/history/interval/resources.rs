//! Resource means retain source coverage and never include unavailable placeholder values.
use crate::history::MetricCoverage;
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceSummary {
    pub mean: Option<f64>,
    /// Highest valid stored frame mean; not an instantaneous device peak.
    pub peak: Option<f64>,
    /// Start of the recorded frame containing that peak, not an intra-frame event time.
    pub peak_ts: Option<i64>,
    /// Sum of saved valid frame means, for ranking; not measured bytes or device time.
    pub recorded_score: Option<f64>,
    pub valid_frames: usize,
    pub partial_frames: usize,
    pub unavailable_frames: usize,
    pub legacy_frames: usize,
    pub invalid_frames: usize,
    /// Raw source-sample coverage when the history format provides it.
    /// Basic memory levels instead use frame counts and have no raw sample counts.
    pub sample_coverage: Option<MetricCoverage>,
}

#[derive(Default)]
pub(super) struct ResourceAcc {
    sum: f64,
    frame_sum: f64,
    weight: u64,
    peak: Option<(f64, i64)>,
    valid: usize,
    partial: usize,
    unavailable: usize,
    legacy: usize,
    invalid: usize,
    coverage: Option<MetricCoverage>,
}

impl ResourceAcc {
    fn value(&mut self, value: f64, weight: u32, partial: bool, ts: i64) {
        self.sum += value * f64::from(weight);
        self.frame_sum += value;
        self.weight += u64::from(weight);
        self.valid += 1;
        self.partial += usize::from(partial);
        if self
            .peak
            .is_none_or(|(peak, at)| value > peak || (value == peak && ts < at))
        {
            self.peak = Some((value, ts));
        }
    }

    pub fn level(&mut self, value: u64, valid: bool, ts: i64) {
        if valid {
            self.value(value as f64, 1, false, ts);
        } else {
            self.unavailable += 1;
        }
    }

    pub fn measured(
        &mut self,
        value: f64,
        coverage: Option<&MetricCoverage>,
        ts: i64,
        maximum: Option<f64>,
    ) {
        let Some(coverage) = coverage else {
            // Legacy numbers do not establish whether the source was available.
            self.legacy += 1;
            return;
        };
        let sane = coverage.valid <= coverage.samples
            && coverage.partial <= coverage.valid
            && (coverage.valid == 0
                || (value.is_finite() && value >= 0. && maximum.is_none_or(|max| value <= max)));
        let total = self.coverage.get_or_insert_with(MetricCoverage::default);
        total.samples = total.samples.saturating_add(coverage.samples);
        total.missing |= coverage.missing;
        if !sane {
            self.invalid += 1;
            self.unavailable += 1;
            return;
        }
        total.valid = total.valid.saturating_add(coverage.valid);
        total.partial = total.partial.saturating_add(coverage.partial);
        if coverage.valid > 0 {
            self.value(value, coverage.valid, coverage.partial > 0, ts);
        } else {
            self.unavailable += 1;
        }
    }

    pub fn finish(&self) -> ResourceSummary {
        ResourceSummary {
            mean: (self.weight > 0).then(|| self.sum / self.weight as f64),
            peak: self.peak.map(|(value, _)| value),
            peak_ts: self.peak.map(|(_, ts)| ts),
            recorded_score: (self.valid > 0).then_some(self.frame_sum),
            valid_frames: self.valid,
            partial_frames: self.partial,
            unavailable_frames: self.unavailable,
            legacy_frames: self.legacy,
            invalid_frames: self.invalid,
            sample_coverage: self.coverage.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn measured_zero_missing_and_legacy_are_distinct_and_mean_uses_valid_samples() {
        let mut acc = ResourceAcc::default();
        let one = MetricCoverage {
            samples: 5,
            valid: 1,
            partial: 0,
            legacy_frames: 0,
            missing: 2,
        };
        let five = MetricCoverage {
            samples: 5,
            valid: 5,
            partial: 0,
            legacy_frames: 0,
            missing: 0,
        };
        let absent = MetricCoverage {
            samples: 5,
            valid: 0,
            partial: 0,
            legacy_frames: 0,
            missing: 4,
        };
        acc.measured(0., Some(&one), 100, None);
        acc.measured(100., Some(&five), 200, None);
        acc.measured(99999., Some(&absent), 300, None);
        acc.measured(99999., None, 400, None);
        let result = acc.finish();
        assert_eq!(result.mean, Some(500. / 6.));
        assert_eq!(result.peak, Some(100.));
        assert_eq!(result.peak_ts, Some(200));
        assert_eq!(
            (
                result.valid_frames,
                result.unavailable_frames,
                result.legacy_frames
            ),
            (2, 1, 1)
        );
        let coverage = result.sample_coverage.unwrap();
        assert_eq!(
            (coverage.samples, coverage.valid, coverage.missing),
            (15, 6, 6)
        );
        let mut zero = ResourceAcc::default();
        zero.measured(0., Some(&one), 100, None);
        assert_eq!(zero.finish().mean, Some(0.));
        let mut missing = ResourceAcc::default();
        missing.measured(0., Some(&absent), 100, None);
        assert_eq!(missing.finish().mean, None);
        let mut placeholder = ResourceAcc::default();
        placeholder.measured(f64::NAN, Some(&absent), 100, Some(100.));
        assert_eq!(placeholder.finish().invalid_frames, 0);
        assert_eq!(placeholder.finish().unavailable_frames, 1);
        let mut legacy = ResourceAcc::default();
        legacy.measured(0., None, 100, None);
        assert_eq!(legacy.finish().mean, None);
        assert_eq!(legacy.finish().legacy_frames, 1);
    }

    #[test]
    fn partial_source_and_invalid_values_do_not_become_complete_measurements() {
        let mut acc = ResourceAcc::default();
        let partial = MetricCoverage {
            samples: 2,
            valid: 2,
            partial: 2,
            legacy_frames: 0,
            missing: 1,
        };
        acc.measured(12., Some(&partial), 100, Some(100.));
        acc.measured(f64::NAN, Some(&partial), 200, Some(100.));
        acc.measured(999., Some(&partial), 300, Some(100.));
        let corrupt = MetricCoverage {
            samples: 1,
            valid: 2,
            ..Default::default()
        };
        acc.measured(20., Some(&corrupt), 400, Some(100.));
        let result = acc.finish();
        assert_eq!(result.mean, Some(12.));
        assert_eq!(
            (
                result.valid_frames,
                result.partial_frames,
                result.invalid_frames
            ),
            (1, 1, 3)
        );
        let mut memory = ResourceAcc::default();
        memory.level(0, true, 100);
        memory.level(1024, true, 200);
        memory.level(99999, false, 300);
        assert_eq!(memory.finish().mean, Some(512.));
        assert!(memory.finish().sample_coverage.is_none());
    }
}
