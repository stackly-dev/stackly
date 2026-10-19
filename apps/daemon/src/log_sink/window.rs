use std::time::{Duration, Instant};

/// Counts dropped lines and remembers file failures over a sliding window,
/// in one second buckets, so `log_sink` can say how many lines it lost in
/// the last minute.
#[derive(Debug)]
pub(super) struct ProblemWindow {
    base: Instant,
    span: Duration,
    /// `(second since base, drops in that second)`, one per second of the
    /// span, reused round robin.
    buckets: Vec<(u64, u64)>,
    last_failure: Option<Instant>,
}

impl ProblemWindow {
    pub(super) fn new(base: Instant, span: Duration) -> Self {
        let seconds = usize::try_from(span.as_secs().max(1)).unwrap_or(usize::MAX);
        Self {
            base,
            span,
            buckets: vec![(0, 0); seconds],
            last_failure: None,
        }
    }

    pub(super) fn record_drop(&mut self, now: Instant) {
        let second = self.second(now);
        let index = self.index(second);
        let bucket = &mut self.buckets[index];
        if bucket.0 != second {
            *bucket = (second, 0);
        }
        bucket.1 = bucket.1.saturating_add(1);
    }

    pub(super) fn record_failure(&mut self, now: Instant) {
        self.last_failure = Some(now);
    }

    /// Lines dropped within the span before `now`.
    pub(super) fn drops(&self, now: Instant) -> u64 {
        let second = self.second(now);
        let span = self.span.as_secs().max(1);
        self.buckets
            .iter()
            .filter(|(at, count)| *count > 0 && *at <= second && second - *at < span)
            .map(|(_, count)| count)
            .fold(0_u64, |total, count| total.saturating_add(*count))
    }

    /// Whether the file failed within the span before `now`.
    pub(super) fn failed(&self, now: Instant) -> bool {
        self.last_failure
            .is_some_and(|at| now.saturating_duration_since(at) < self.span)
    }

    fn second(&self, now: Instant) -> u64 {
        now.saturating_duration_since(self.base).as_secs()
    }

    fn index(&self, second: u64) -> usize {
        let len = u64::try_from(self.buckets.len()).unwrap_or(u64::MAX);
        usize::try_from(second % len).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINUTE: Duration = Duration::from_secs(60);

    #[test]
    fn drops_count_only_within_the_last_minute() {
        let base = Instant::now();
        let mut window = ProblemWindow::new(base, MINUTE);

        window.record_drop(base);
        window.record_drop(base + Duration::from_secs(30));
        window.record_drop(base + Duration::from_secs(30));

        assert_eq!(window.drops(base + Duration::from_secs(30)), 3);
        assert_eq!(window.drops(base + Duration::from_secs(59)), 3);
        assert_eq!(window.drops(base + Duration::from_secs(60)), 2);
        assert_eq!(window.drops(base + Duration::from_secs(90)), 0);
    }

    #[test]
    fn a_reused_bucket_forgets_its_old_second() {
        let base = Instant::now();
        let mut window = ProblemWindow::new(base, MINUTE);
        window.record_drop(base + Duration::from_secs(5));

        window.record_drop(base + Duration::from_secs(65));

        assert_eq!(window.drops(base + Duration::from_secs(65)), 1);
    }

    #[test]
    fn a_file_failure_counts_for_one_minute() {
        let base = Instant::now();
        let mut window = ProblemWindow::new(base, MINUTE);
        assert!(!window.failed(base));

        window.record_failure(base + Duration::from_secs(10));

        assert!(window.failed(base + Duration::from_secs(69)));
        assert!(!window.failed(base + Duration::from_secs(70)));
    }
}
