use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// A shared flag that asks a long running step to stop at its next chunk
/// boundary: the download of a `Fetcher` and the runtime extractor check
/// it between chunks (spec 0018). Clones share one flag.
#[derive(Debug, Clone, Default)]
pub struct StopFlag(Arc<AtomicBool>);

impl StopFlag {
    /// A flag that is not raised.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Raises the flag for every clone.
    pub fn stop(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    /// Returns true once any clone raised the flag.
    #[must_use]
    pub fn is_stopped(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_raised_flag_is_seen_by_every_clone() {
        let flag = StopFlag::new();
        let clone = flag.clone();
        assert!(!clone.is_stopped());

        flag.stop();

        assert!(clone.is_stopped());
        assert!(flag.is_stopped());
    }
}
