use std::fmt;
use std::str::FromStr;

use uuid::Uuid;

use crate::error::{ValueError, ValueKind};
use crate::rule::Rule;
use crate::text::serde_as_text;

/// Length of a hyphenated UUID, the only text form an ID accepts.
const HYPHENATED_LEN: usize = 36;
/// Number of hex characters in a [`ShortKey`].
const SHORT_KEY_LEN: usize = 8;

/// Defines a newtype over a UUID version 7 with the same rules for every
/// kind of ID: only the daemon calls `generate()`, the text form is the
/// lowercase hyphenated UUID, parsing accepts any letter case, and serde
/// goes through the text form, so it rejects every version other than 7.
macro_rules! uuid_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(Uuid);

        impl $name {
            /// Creates a new, time ordered ID from the current system time.
            #[must_use]
            pub fn generate() -> Self {
                Self(Uuid::now_v7())
            }

            /// Wraps an existing UUID, for example one read back from the
            /// registry.
            ///
            /// # Errors
            ///
            /// Returns [`InvalidId::WrongVersion`] when the UUID is not
            /// version 7.
            pub fn from_uuid(uuid: Uuid) -> Result<Self, InvalidId> {
                match uuid.get_version_num() {
                    7 => Ok(Self(uuid)),
                    other => Err(InvalidId::WrongVersion(other)),
                }
            }

            /// Returns the underlying UUID.
            #[must_use]
            pub const fn as_uuid(&self) -> Uuid {
                self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Display::fmt(&self.0.hyphenated(), f)
            }
        }

        impl FromStr for $name {
            type Err = InvalidId;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                if s.len() != HYPHENATED_LEN {
                    return Err(InvalidId::Malformed);
                }
                let uuid = Uuid::try_parse(s).map_err(|_| InvalidId::Malformed)?;
                Self::from_uuid(uuid)
            }
        }

        serde_as_text!($name => "a hyphenated UUID version 7");
    };
}

uuid_id! {
    /// Stable identifier of an instance, always a UUID version 7.
    ///
    /// Only the daemon creates new IDs. The text form is the lowercase
    /// hyphenated UUID, and parsing accepts any letter case.
    ///
    /// ```
    /// use stackly_domain::InstanceId;
    ///
    /// let id = InstanceId::generate();
    /// let parsed: InstanceId = id.to_string().to_uppercase().parse()?;
    /// assert_eq!(parsed, id);
    /// # Ok::<(), stackly_domain::InvalidId>(())
    /// ```
    InstanceId
}

uuid_id! {
    /// Stable identifier of an installed runtime, always a UUID version 7.
    RuntimeId
}

uuid_id! {
    /// Stable identifier of an environment, always a UUID version 7.
    EnvironmentId
}

uuid_id! {
    /// Stable identifier of an operation, always a UUID version 7.
    OperationId
}

uuid_id! {
    /// Stable identifier of a lease on an instance, always a UUID version 7.
    LeaseId
}

uuid_id! {
    /// Stable identifier of a backup, always a UUID version 7.
    ///
    /// Feature 4 uses it only to name recovery snapshots that keep a runtime
    /// in use; backups themselves come with feature 49.
    BackupId
}

uuid_id! {
    /// Stable identifier of a provider source (a catalog the daemon loads),
    /// always a UUID version 7.
    ProviderSourceId
}

impl InstanceId {
    /// Returns the short key of this instance: the last 8 hex characters of
    /// its text form, which come from the random part of the UUID.
    ///
    /// The short key names the instance's run folder under the runtime root,
    /// where socket paths must stay short.
    ///
    /// ```
    /// use stackly_domain::InstanceId;
    ///
    /// let id: InstanceId = "01928c3e-6f1a-7b2c-8d4e-5f60718293a4".parse()?;
    /// assert_eq!(id.short_key().to_string(), "718293a4");
    /// # Ok::<(), stackly_domain::InvalidId>(())
    /// ```
    #[must_use]
    pub fn short_key(&self) -> ShortKey {
        let [.., a, b, c, d] = *self.0.as_bytes();
        ShortKey(u32::from_be_bytes([a, b, c, d]))
    }
}

/// Error returned when text or a UUID is not a valid ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum InvalidId {
    /// The text is not a hyphenated UUID.
    #[error("ID is not a hyphenated UUID")]
    Malformed,
    /// The UUID is well formed but not version 7.
    #[error("ID must be a UUID version 7, found version {0}")]
    WrongVersion(usize),
}

impl InvalidId {
    /// Returns the business rule this error enforces. ID rules are not
    /// business rules, so this is always `None`.
    #[must_use]
    pub const fn rule(&self) -> Option<Rule> {
        None
    }
}

/// The 8 lowercase hex characters that name an instance's run folder.
///
/// Derived from an [`InstanceId`] with [`InstanceId::short_key`], never
/// chosen freely. Two instances can share a short key only by chance (32
/// random bits), and creating an instance retries with a new ID when that
/// happens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ShortKey(u32);

impl fmt::Display for ShortKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:08x}", self.0)
    }
}

impl FromStr for ShortKey {
    type Err = ValueError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let invalid = || ValueError::new(ValueKind::ShortKey, "must be 8 lowercase hex characters");
        let is_lower_hex = |b: &u8| b.is_ascii_digit() || (b'a'..=b'f').contains(b);
        if s.len() != SHORT_KEY_LEN || !s.as_bytes().iter().all(is_lower_hex) {
            return Err(invalid());
        }
        u32::from_str_radix(s, 16).map(Self).map_err(|_| invalid())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const V7: &str = "01928c3e-6f1a-7b2c-8d4e-5f60718293a4";

    #[test]
    fn generated_ids_are_version_7_and_round_trip_through_text() {
        let id = InstanceId::generate();

        assert_eq!(id.as_uuid().get_version_num(), 7);
        assert_eq!(id.to_string().parse::<InstanceId>().unwrap(), id);
    }

    #[test]
    fn text_form_is_lowercase_and_parsing_ignores_case() {
        let id: EnvironmentId = V7.to_uppercase().parse().unwrap();

        assert_eq!(id.to_string(), V7);
    }

    #[test]
    fn ids_that_are_not_version_7_are_rejected() {
        let v4 = "9f1c2d3e-4b5a-4c6d-8e7f-0a1b2c3d4e5f";

        assert_eq!(v4.parse::<InstanceId>(), Err(InvalidId::WrongVersion(4)));
        assert_eq!(
            EnvironmentId::from_uuid(Uuid::nil()),
            Err(InvalidId::WrongVersion(0))
        );
    }

    #[test]
    fn text_other_than_a_hyphenated_uuid_is_malformed() {
        let braced = format!("{{{V7}}}");
        let simple = V7.replace('-', "");
        let cases: [&str; 4] = ["", "not-an-id", &simple, &braced];
        for text in cases {
            assert_eq!(
                text.parse::<InstanceId>(),
                Err(InvalidId::Malformed),
                "{text}"
            );
        }
    }

    #[test]
    fn every_id_type_serializes_as_its_lowercase_text_form() {
        let upper = format!("\"{}\"", V7.to_uppercase());

        let runtime: RuntimeId = serde_json::from_str(&upper).unwrap();
        let operation: OperationId = serde_json::from_str(&upper).unwrap();
        let lease: LeaseId = serde_json::from_str(&upper).unwrap();
        let backup: BackupId = serde_json::from_str(&upper).unwrap();
        let instance: InstanceId = serde_json::from_str(&upper).unwrap();
        let environment: EnvironmentId = serde_json::from_str(&upper).unwrap();

        let expected = format!("\"{V7}\"");
        assert_eq!(serde_json::to_string(&runtime).unwrap(), expected);
        assert_eq!(serde_json::to_string(&operation).unwrap(), expected);
        assert_eq!(serde_json::to_string(&lease).unwrap(), expected);
        assert_eq!(serde_json::to_string(&backup).unwrap(), expected);
        assert_eq!(serde_json::to_string(&instance).unwrap(), expected);
        assert_eq!(serde_json::to_string(&environment).unwrap(), expected);
    }

    #[test]
    fn serde_rejects_ids_that_are_not_version_7_or_not_text() {
        let v4 = "\"9f1c2d3e-4b5a-4c6d-8e7f-0a1b2c3d4e5f\"";

        let err = serde_json::from_str::<OperationId>(v4).unwrap_err();
        assert!(err.to_string().contains("found version 4"), "{err}");
        let err = serde_json::from_str::<RuntimeId>("42").unwrap_err();
        assert!(err.to_string().contains("UUID version 7"), "{err}");
        assert_eq!(InvalidId::Malformed.rule(), None);
    }

    #[test]
    fn short_key_is_the_last_eight_hex_characters_of_the_id() {
        for _ in 0..100 {
            let id = InstanceId::generate();
            let text = id.to_string();

            assert_eq!(id.short_key().to_string(), text[text.len() - 8..]);
        }
    }

    #[test]
    fn short_key_parses_only_eight_lowercase_hex_characters() {
        let key: ShortKey = "0000a1ff".parse().unwrap();

        assert_eq!(key.to_string(), "0000a1ff");
        for text in [
            "",
            "0000a1f",
            "0000a1ff0",
            "0000A1FF",
            "0000a1fg",
            "+000a1ff",
        ] {
            let err = text.parse::<ShortKey>().unwrap_err();
            assert_eq!(err.kind(), ValueKind::ShortKey, "{text}");
        }
    }
}
