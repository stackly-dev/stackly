//! The seam where secret values leave failure messages before the journal
//! stores them (spec 0007, "Kegagalan dan error"; spec 0011 `Redactor`).

/// Removes secret values from text the engine stores: a failure message and
/// the values of its details. Spec 0011's `Redactor` implements it; the
/// daemon hands it to the engine in [`EngineDeps`](super::EngineDeps).
pub trait Redact: Send + Sync + 'static {
    /// Returns `text` with every known secret value replaced.
    fn redact(&self, text: &str) -> String;
}

/// Keeps text as it is, until feature 11 provides its `Redactor`. Work
/// errors never carry secret values today, because no secret store exists
/// yet.
// provisional: F11
#[derive(Debug, Clone, Copy, Default)]
pub struct NoSecretsYet;

impl Redact for NoSecretsYet {
    fn redact(&self, text: &str) -> String {
        text.to_owned()
    }
}
