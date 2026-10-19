//! The reference to a secret that settings hold instead of its value, and
//! the resolved values of one action.

use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use super::names::SettingName;

/// Longest reference, in bytes.
const SECRET_REF_MAX: usize = 256;

// provisional: F11 (replaced by the SecretRef of spec 0011)
/// An opaque reference to a secret in the secret store: 1 to 256 bytes of
/// printable ASCII without spaces. It never holds the secret's value.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SecretRef(String);

impl SecretRef {
    /// Returns the reference text, for the secret store to resolve.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SecretRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SecretRef({})", self.0)
    }
}

impl fmt::Display for SecretRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Error returned when text is not a valid [`SecretRef`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("secret reference must be 1 to 256 bytes of printable ASCII without spaces")]
#[non_exhaustive]
pub struct SecretRefError;

impl FromStr for SecretRef {
    type Err = SecretRefError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if (1..=SECRET_REF_MAX).contains(&s.len()) && s.bytes().all(|b| (0x21..=0x7e).contains(&b))
        {
            Ok(Self(s.to_owned()))
        } else {
            Err(SecretRefError)
        }
    }
}

// provisional: F11 (replaced by the ResolvedSecrets of spec 0011)
/// The secret values of one action, by secret field, as
/// `SecretService::resolve_for_start` of spec 0011 returns them.
/// [`RenderedContent::materialize`](super::RenderedContent::materialize)
/// reads it. `Debug` shows only the field names, never a value.
pub struct ResolvedSecrets(BTreeMap<SettingName, Vec<u8>>);

impl ResolvedSecrets {
    /// Holds the given value for each field. Only tests build one this way;
    /// the daemon gets it from the secret store of spec 0011.
    #[must_use]
    pub fn for_tests(values: impl IntoIterator<Item = (SettingName, Vec<u8>)>) -> Self {
        Self(values.into_iter().collect())
    }

    /// Returns the exact bytes of a field's secret.
    pub(crate) fn get(&self, field: &SettingName) -> Option<&[u8]> {
        self.0.get(field).map(Vec::as_slice)
    }
}

impl fmt::Debug for ResolvedSecrets {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_set()
            .entries(self.0.keys().map(SettingName::as_str))
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn references_are_printable_ascii_without_spaces() {
        let reference: SecretRef = "fixture:authToken".parse().unwrap();
        assert_eq!(reference.as_str(), "fixture:authToken");
        assert!("".parse::<SecretRef>().is_err());
        assert!("a b".parse::<SecretRef>().is_err());
        assert!("ü".parse::<SecretRef>().is_err());
        assert!("x".repeat(256).parse::<SecretRef>().is_ok());
        assert!("x".repeat(257).parse::<SecretRef>().is_err());
        assert!(
            "local:instance/0192f000-0000-7000-8000-00000000000a/auth-token"
                .parse::<SecretRef>()
                .is_ok()
        );
    }

    #[test]
    fn resolved_secrets_show_only_their_fields() {
        let field: SettingName = "authToken".parse().unwrap();
        let secrets = ResolvedSecrets::for_tests([(field.clone(), b"hunter2".to_vec())]);

        assert_eq!(secrets.get(&field), Some(&b"hunter2"[..]));
        assert_eq!(secrets.get(&"other".parse().unwrap()), None);
        assert_eq!(format!("{secrets:?}"), "{\"authToken\"}");
    }
}
