//! Retry optional driver queries after a short backoff. A failed query must
//! neither permanently disable telemetry nor keep stale readings alive.

use std::time::{Duration, Instant};

#[derive(Default)]
pub(super) struct Retry {
    after: Option<Instant>,
}

impl Retry {
    pub fn query<T>(&mut self, now: Instant, read: impl FnOnce() -> Option<T>) -> Option<T> {
        if self.after.is_some_and(|after| now < after) {
            return None;
        }
        let value = read();
        self.after = value.is_none().then(|| now + Duration::from_secs(5));
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failure_backs_off_without_losing_recovery_or_reusing_stale_data() {
        let now = Instant::now();
        let mut retry = Retry::default();
        assert_eq!(retry.query(now, || Some(35)), Some(35));
        assert_eq!(retry.query::<u32>(now, || None), None);
        for offset in [0, 1, 4] {
            assert_eq!(
                retry.query::<u32>(now + Duration::from_secs(offset), || panic!("backoff")),
                None
            );
        }
        assert_eq!(
            retry.query(now + Duration::from_secs(5), || Some(0)),
            Some(0)
        );
        assert_eq!(
            retry.query(now + Duration::from_secs(6), || Some(36)),
            Some(36)
        );
    }
}
