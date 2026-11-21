use std::collections::VecDeque;

use super::{MIN_SPAN_MS, RETAIN_MS};

#[derive(Default)]
pub(super) struct Series {
    origin: i64,
    points: VecDeque<Point>,
}

struct Point {
    ms: u32,
    count: u32,
}

impl Series {
    pub fn record(&mut self, now: i64, count: u32) {
        let cutoff = now.saturating_sub(RETAIN_MS);
        while self
            .points
            .front()
            .is_some_and(|p| self.origin + i64::from(p.ms) < cutoff)
        {
            self.points.pop_front();
        }
        if let Some(first) = self.points.front() {
            // Rebase long-lived series before their millisecond offsets overflow.
            if now.abs_diff(self.origin) > u64::from(u32::MAX) {
                let shift = first.ms;
                self.origin += i64::from(shift);
                for p in &mut self.points {
                    p.ms -= shift;
                }
            }
        } else {
            self.origin = now;
        }
        self.points.push_back(Point {
            ms: (now - self.origin) as u32,
            count,
        });
    }

    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    #[cfg(test)]
    pub(super) fn point_bytes(&self) -> usize {
        self.points.capacity() * size_of::<Point>()
    }

    pub fn start(&self) -> i64 {
        self.origin + i64::from(self.points[0].ms)
    }

    pub fn span(&self) -> i64 {
        i64::from(self.points.back().unwrap().ms - self.points[0].ms)
    }

    pub fn growing(&self, required_rise: impl FnOnce(u32) -> u32) -> Option<u32> {
        let span = self.span();
        if span < MIN_SPAN_MS {
            return None;
        }
        let first = &self.points[0];
        let boundary = |segment: u32| {
            self.points
                .partition_point(|p| (p.ms - first.ms) * 5 < span as u32 * segment)
        };
        let floor = |start, end| self.points.range(start..end).map(|p| p.count).min();
        let mut start = boundary(1);
        let first_floor = floor(0, start)?;
        let minimum = required_rise(first_floor);
        // The latest count bounds the last floor. Small fluctuations need no further scan.
        if self.points.back()?.count.checked_sub(first_floor)? < minimum {
            return None;
        }
        let mut previous = first_floor;
        for segment in 2..=5 {
            let end = if segment == 5 {
                self.points.len()
            } else {
                boundary(segment)
            };
            let next = floor(start, end)?;
            if next < previous {
                return None;
            }
            previous = next;
            start = end;
        }
        let rise = previous - first_floor;
        if rise < minimum {
            return None;
        }

        // Use offsets on both axes to keep large absolute counts out of the fit.
        let (mut sx, mut sy, mut sxx, mut syy, mut sxy) = (0., 0., 0., 0., 0.);
        for p in &self.points {
            let x = f64::from(p.ms - first.ms);
            let y = f64::from(p.count) - f64::from(first.count);
            sx += x;
            sy += y;
            sxx += x * x;
            syy += y * y;
            sxy += x * y;
        }
        let n = self.points.len() as f64;
        let covariance = n * sxy - sx * sy;
        let variance_product = (n * sxx - sx * sx) * (n * syy - sy * sy);
        (covariance > 0.
            && variance_product > 0.
            && covariance * covariance >= 0.8 * variance_product)
            .then_some(rise)
    }

    pub fn trend(&self) -> Vec<u64> {
        let first = &self.points[0];
        let slots = self.points.len().min(48);
        if slots == 1 {
            return vec![u64::from(first.count)];
        }
        // Pick the nearest observation to each equally spaced time, including both ends.
        let mut index = 0;
        (0..slots)
            .map(|i| {
                let target = first.ms + (self.span() * i as i64 / (slots - 1) as i64) as u32;
                while let Some(next) = self.points.get(index + 1) {
                    if next.ms.abs_diff(target) >= self.points[index].ms.abs_diff(target) {
                        break;
                    }
                    index += 1;
                }
                u64::from(self.points[index].count)
            })
            .collect()
    }
}
