//! The last guard before a spawn: no secret value in argv, where every
//! process of the user can read it.

use std::fmt;

use stackly_platform::SCRUB_ANYWHERE_LEN;

use crate::provider::SettingName;
use crate::secret::ResolvedSecrets;

/// Which rule an argument broke in [`ensure_not_in_argv`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SecretInArgvReason {
    /// The argument is exactly a value.
    EqualsValue,
    /// The argument ends with `=` and a value, as in `--password=<value>`.
    AssignsValue,
    /// The argument holds a value of
    /// [`SCRUB_ANYWHERE_LEN`](stackly_platform::SCRUB_ANYWHERE_LEN) bytes
    /// or more.
    ContainsValue,
}

impl SecretInArgvReason {
    /// Returns the stable snake_case code, for the `reason` detail of
    /// `secret.delivery_rejected`.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::EqualsValue => "equals_value",
            Self::AssignsValue => "assigns_value",
            Self::ContainsValue => "contains_value",
        }
    }
}

impl fmt::Display for SecretInArgvReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_code())
    }
}

/// Error returned when an argument would carry a secret value. The daemon
/// maps it to `secret.delivery_rejected`. It names the argument's position
/// and the secret field, never the value.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("argument {index} would carry the secret {field} ({reason})")]
#[non_exhaustive]
pub struct SecretInArgv {
    /// The argument's position in argv, the program excluded, from 0.
    pub index: usize,
    /// The secret field whose value it carries.
    pub field: SettingName,
    /// The rule it broke.
    pub reason: SecretInArgvReason,
}

/// Rejects arguments that would carry a secret value of the action: one
/// that is exactly a value (however short), one that ends with `=` and a
/// value, and one that holds a value of
/// [`SCRUB_ANYWHERE_LEN`](stackly_platform::SCRUB_ANYWHERE_LEN) bytes or
/// more (AC-12). A shorter value inside a longer argument passes, so a
/// path holding `postgres` is not mistaken for a secret.
///
/// # Errors
///
/// Returns [`SecretInArgv`] for the first argument that breaks a rule.
pub fn ensure_not_in_argv<S: AsRef<str>>(
    args: &[S],
    secrets: &ResolvedSecrets,
) -> Result<(), SecretInArgv> {
    for (index, arg) in args.iter().enumerate() {
        let arg = arg.as_ref();
        for (field, value) in secrets.iter() {
            let value = value.expose();
            let reason = if arg == value {
                SecretInArgvReason::EqualsValue
            } else if arg
                .strip_suffix(value)
                .is_some_and(|head| head.ends_with('='))
            {
                SecretInArgvReason::AssignsValue
            } else if value.len() >= SCRUB_ANYWHERE_LEN && arg.contains(value) {
                SecretInArgvReason::ContainsValue
            } else {
                continue;
            };
            return Err(SecretInArgv {
                index,
                field: field.clone(),
                reason,
            });
        }
    }
    Ok(())
}
