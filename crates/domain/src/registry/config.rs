//! What the registry keeps for an instance's configuration (spec 0005,
//! "Tambahan fitur 10"): one row per config revision, and the instance's
//! pointer to the active one.
//!
//! Feature 10 writes the first revision when it creates an instance;
//! feature 29 adds the rules for the revisions after it.

use std::fmt;
use std::str::FromStr;

use jiff::Timestamp;

use crate::code::code_enum;
use crate::error::{ValueError, ValueKind};
use crate::id::{InstanceId, OperationId};
use crate::registry::ConfigRevisionId;
use crate::value::Sha256Digest;

/// The settings values of one config revision, as canonical JSON object
/// text: keys sorted, no whitespace. A secret field holds its reference
/// text, never a value.
///
/// Core builds it from the provider's settings values with `serde_json` and
/// reads it back through the settings schema. The domain has no JSON
/// parser, so it checks only that the text is not empty, starts with `{`,
/// and ends with `}`; the `config_values` column's JSON object check is the
/// last guard.
///
/// ```
/// use stackly_domain::ConfigValuesJson;
///
/// let values = ConfigValuesJson::new(r#"{"port":5432}"#.to_owned())?;
/// assert_eq!(values.as_str(), r#"{"port":5432}"#);
/// assert!(ConfigValuesJson::new("[]".to_owned()).is_err());
/// # Ok::<(), stackly_domain::ValueError>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ConfigValuesJson(String);

impl ConfigValuesJson {
    /// Wraps the JSON object text of the values.
    ///
    /// # Errors
    ///
    /// Returns a [`ValueError`] when the text does not start with `{` and
    /// end with `}`.
    pub fn new(text: String) -> Result<Self, ValueError> {
        if text.len() >= 2 && text.starts_with('{') && text.ends_with('}') {
            Ok(Self(text))
        } else {
            Err(ValueError::new(
                ValueKind::ConfigValuesJson,
                "must be JSON object text, from `{` to `}`",
            ))
        }
    }

    /// Returns the JSON text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ConfigValuesJson {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for ConfigValuesJson {
    type Err = ValueError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s.to_owned())
    }
}

code_enum! {
    /// Where a config revision stands, an open code set in the registry.
    /// Feature 10 writes only `active`; feature 29 adds the codes of
    /// revisions that are proposed, superseded, or rolled back. A stored
    /// code this build does not know reads as `Corrupt`.
    pub enum ConfigRevisionStatus: ConfigRevisionStatus {
        /// The revision the instance runs with.
        Active => "active",
    }
}

/// One config revision of an instance: the `config_revisions` row.
///
/// It is not part of the [`Instance`](crate::Instance) aggregate: writing
/// it never raises the instance's revision, and the instance points to the
/// active one through a column the aggregate does not carry.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ConfigRevisionRecord {
    /// The revision's ID.
    pub id: ConfigRevisionId,
    /// The instance it configures.
    pub instance: InstanceId,
    /// The settings values.
    pub values: ConfigValuesJson,
    /// The SHA-256 of the rendered config files, once rendered.
    pub rendered_sha256: Option<Sha256Digest>,
    /// The revision it follows, a revision of the same instance.
    pub previous: Option<ConfigRevisionId>,
    /// Where it stands.
    pub status: ConfigRevisionStatus,
    /// The operation that wrote it, if any.
    pub created_by_operation: Option<OperationId>,
    /// When it was written.
    pub created_at: Timestamp,
}

impl ConfigRevisionRecord {
    /// The first revision of `instance`: `active`, following none, not
    /// rendered yet, written by `operation` `at`.
    #[must_use]
    pub const fn first(
        id: ConfigRevisionId,
        instance: InstanceId,
        values: ConfigValuesJson,
        operation: Option<OperationId>,
        at: Timestamp,
    ) -> Self {
        Self {
            id,
            instance,
            values,
            rendered_sha256: None,
            previous: None,
            status: ConfigRevisionStatus::Active,
            created_by_operation: operation,
            created_at: at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_values_take_object_text_and_refuse_anything_else() {
        for text in ["{}", r#"{"a":1}"#, "{x}"] {
            assert_eq!(
                ConfigValuesJson::new(text.to_owned()).unwrap().as_str(),
                text
            );
        }
        for text in ["", "{", "}", "[]", " {}", "{} ", "null"] {
            let err = ConfigValuesJson::new(text.to_owned()).unwrap_err();
            assert_eq!(err.kind(), ValueKind::ConfigValuesJson, "{text:?}");
        }
        assert_eq!(
            r#"{"a":1}"#.parse::<ConfigValuesJson>().unwrap().to_string(),
            r#"{"a":1}"#
        );
    }

    #[test]
    fn config_revision_status_knows_only_active_for_now() {
        assert_eq!(ConfigRevisionStatus::ALL, &[ConfigRevisionStatus::Active]);
        assert_eq!(
            "active".parse::<ConfigRevisionStatus>(),
            Ok(ConfigRevisionStatus::Active)
        );
        assert_eq!(
            "superseded"
                .parse::<ConfigRevisionStatus>()
                .unwrap_err()
                .kind(),
            ValueKind::ConfigRevisionStatus
        );
    }

    #[test]
    fn a_first_revision_is_active_and_follows_none() {
        let id = ConfigRevisionId::generate();
        let instance = InstanceId::generate();
        let values = ConfigValuesJson::new("{}".to_owned()).unwrap();
        let record =
            ConfigRevisionRecord::first(id, instance, values.clone(), None, Timestamp::UNIX_EPOCH);

        assert_eq!(record.id, id);
        assert_eq!(record.instance, instance);
        assert_eq!(record.values, values);
        assert_eq!(record.status, ConfigRevisionStatus::Active);
        assert_eq!((record.previous, record.rendered_sha256), (None, None));
        assert_eq!(record.created_by_operation, None);
        assert_eq!(record.created_at, Timestamp::UNIX_EPOCH);
    }
}
