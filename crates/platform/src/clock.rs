use jiff::Timestamp;

/// The source of the current time. The domain never reads the clock itself;
/// use cases pass the time in.
pub trait Clock: Send + Sync {
    /// Returns the current time in UTC.
    fn now(&self) -> Timestamp;
}

/// The system clock.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        Timestamp::now()
    }
}
