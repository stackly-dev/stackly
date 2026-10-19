//! The seam where secret values leave failure messages before the journal
//! stores them (spec 0007, "Kegagalan dan error"; spec 0011 `Redactor`).

/// Removes secret values from text the engine stores: a failure message and
/// the values of its details. [`Redactor`](crate::secret::Redactor)
/// implements it; the daemon hands its shared redactor to the engine in
/// [`EngineDeps`](super::EngineDeps).
pub trait Redact: Send + Sync + 'static {
    /// Returns `text` with every known secret value replaced.
    fn redact(&self, text: &str) -> String;
}
