//! The reference to a secret that settings hold instead of its value.

use std::fmt;
use std::str::FromStr;

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
    }
}
