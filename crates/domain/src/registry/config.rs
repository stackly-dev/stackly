//! What the registry keeps for an instance's configuration (spec 0005,
//! "Tambahan fitur 10"): one row per config revision, and the instance's
//! pointer to the active one.
//!
//! Feature 10 writes the first revision when it creates an instance;
//! feature 29 adds the rules for the revisions after it (spec 0029): a
//! number per instance, a snapshot of the IP endpoints' port requests, the
//! origin, and `ready_at`, which makes a revision a last known good.

use std::collections::BTreeMap;
use std::fmt;
use std::num::NonZeroU64;
use std::str::FromStr;

use jiff::Timestamp;

use crate::code::code_enum;
use crate::endpoint::{Endpoint, PortRequest};
use crate::error::{ValueError, ValueKind};
use crate::id::{InstanceId, OperationId};
use crate::instance::Instance;
use crate::registry::ConfigRevisionId;
use crate::value::{EndpointName, Sha256Digest};

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
    /// The active one is the revision the instance runs with; every older
    /// one is superseded (spec 0029). A stored code this build does not know
    /// reads as `Corrupt`.
    pub enum ConfigRevisionStatus: ConfigRevisionStatus {
        /// The revision the instance runs with.
        Active => "active",
        /// A revision a later one replaced.
        Superseded => "superseded",
    }
}

code_enum! {
    /// How a config revision came to be (spec 0029), an open code set in
    /// the registry. A stored code this build does not know reads as
    /// `Corrupt`.
    pub enum ConfigRevisionOrigin: ConfigRevisionOrigin {
        /// The first revision, from the provider defaults and the create
        /// form.
        Created => "created",
        /// New settings from the Configuration page or `config apply`.
        Applied => "applied",
        /// New ports from `change_endpoints`.
        Endpoints => "endpoints",
        /// A roll back to the last known good revision.
        Rollback => "rollback",
        /// A restore of an older revision from the history.
        Restored => "restored",
    }
}

/// The number of a config revision within its instance: 1 for the first,
/// then one more than the active revision for every next one, so it is
/// never reused even after prune deletes old rows (spec 0029).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ConfigRevisionNumber(NonZeroU64);

impl ConfigRevisionNumber {
    /// The number of an instance's first revision.
    pub const FIRST: Self = Self(NonZeroU64::MIN);

    /// Wraps a stored number.
    ///
    /// # Errors
    ///
    /// Returns a [`ValueError`] for 0, because numbers start at 1.
    pub const fn new(number: u64) -> Result<Self, ValueError> {
        match NonZeroU64::new(number) {
            Some(number) => Ok(Self(number)),
            None => Err(ValueError::new(
                ValueKind::ConfigRevisionNumber,
                "must be at least 1",
            )),
        }
    }

    /// Returns the number.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0.get()
    }

    /// Returns the number after this one.
    ///
    /// # Panics
    ///
    /// Panics at `u64::MAX`, which no instance reaches one apply at a
    /// time.
    #[must_use]
    pub const fn next(self) -> Self {
        Self(
            self.0
                .checked_add(1)
                .expect("a config revision number never reaches u64::MAX"),
        )
    }
}

impl fmt::Display for ConfigRevisionNumber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

/// The port request of every IP endpoint of an instance, by endpoint name:
/// the endpoint snapshot a config revision carries (spec 0029). Unix
/// endpoints have no port and are never in it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct EndpointRequests(BTreeMap<EndpointName, PortRequest>);

impl EndpointRequests {
    /// Wraps a map of endpoint names to port requests.
    #[must_use]
    pub const fn new(requests: BTreeMap<EndpointName, PortRequest>) -> Self {
        Self(requests)
    }

    /// The port requests of the IP endpoints of `instance`.
    #[must_use]
    pub fn of(instance: &Instance) -> Self {
        Self(
            instance
                .endpoints()
                .iter()
                .filter_map(|endpoint| match endpoint {
                    Endpoint::Ip(ip) => Some((ip.name().clone(), ip.request())),
                    Endpoint::Unix(_) => None,
                })
                .collect(),
        )
    }

    /// Returns the request of the endpoint named `name`.
    #[must_use]
    pub fn get(&self, name: &EndpointName) -> Option<PortRequest> {
        self.0.get(name).copied()
    }

    /// Returns every endpoint name and request, ordered by name.
    pub fn iter(&self) -> impl Iterator<Item = (&EndpointName, PortRequest)> {
        self.0.iter().map(|(name, request)| (name, *request))
    }

    /// Returns how many endpoints the snapshot holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns whether the snapshot holds no endpoint.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// One config revision of an instance: the `config_revisions` row.
///
/// It is not part of the [`Instance`] aggregate. Inserting the first one
/// never raises the instance's revision; activating a later one goes
/// through [`WriteOp::ActivateConfigRevision`](crate::WriteOp::ActivateConfigRevision),
/// which updates the aggregate in the same op (spec 0029).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ConfigRevisionRecord {
    /// The revision's ID.
    pub id: ConfigRevisionId,
    /// The instance it configures.
    pub instance: InstanceId,
    /// Its number within the instance.
    pub number: ConfigRevisionNumber,
    /// The settings values.
    pub values: ConfigValuesJson,
    /// The port requests of the IP endpoints.
    pub endpoints: EndpointRequests,
    /// The SHA-256 of the rendered config files, once rendered.
    pub rendered_sha256: Option<Sha256Digest>,
    /// The revision it follows, a revision of the same instance. Prune can
    /// leave it empty, so the history is ordered by `number`.
    pub previous: Option<ConfigRevisionId>,
    /// Where it stands.
    pub status: ConfigRevisionStatus,
    /// How it came to be.
    pub origin: ConfigRevisionOrigin,
    /// The number of the revision it copies, for `rollback` and
    /// `restored`.
    pub source_number: Option<ConfigRevisionNumber>,
    /// When an instance first reached ready on it, which makes it a last
    /// known good.
    pub ready_at: Option<Timestamp>,
    /// The operation that wrote it, if any.
    pub created_by_operation: Option<OperationId>,
    /// When it was written.
    pub created_at: Timestamp,
}

impl ConfigRevisionRecord {
    /// The first revision of `instance`: number 1, `active`, `created`,
    /// following none, not rendered or ready yet, written by `operation`
    /// `at`.
    #[must_use]
    pub const fn first(
        id: ConfigRevisionId,
        instance: InstanceId,
        values: ConfigValuesJson,
        endpoints: EndpointRequests,
        operation: Option<OperationId>,
        at: Timestamp,
    ) -> Self {
        Self {
            id,
            instance,
            number: ConfigRevisionNumber::FIRST,
            values,
            endpoints,
            rendered_sha256: None,
            previous: None,
            status: ConfigRevisionStatus::Active,
            origin: ConfigRevisionOrigin::Created,
            source_number: None,
            ready_at: None,
            created_by_operation: operation,
            created_at: at,
        }
    }

    /// The revision that follows `active`: the next number, `active`,
    /// following `active`, not rendered or ready yet. `source` is the
    /// number it copies, for `rollback` and `restored`.
    #[must_use]
    #[expect(clippy::too_many_arguments, reason = "one argument per column it sets")]
    pub const fn following(
        active: &Self,
        id: ConfigRevisionId,
        values: ConfigValuesJson,
        endpoints: EndpointRequests,
        origin: ConfigRevisionOrigin,
        source: Option<ConfigRevisionNumber>,
        operation: Option<OperationId>,
        at: Timestamp,
    ) -> Self {
        Self {
            id,
            instance: active.instance,
            number: active.number.next(),
            values,
            endpoints,
            rendered_sha256: None,
            previous: Some(active.id),
            status: ConfigRevisionStatus::Active,
            origin,
            source_number: source,
            ready_at: None,
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
    fn config_revision_status_knows_active_and_superseded() {
        assert_eq!(
            ConfigRevisionStatus::ALL,
            &[
                ConfigRevisionStatus::Active,
                ConfigRevisionStatus::Superseded
            ]
        );
        assert_eq!(
            "superseded".parse::<ConfigRevisionStatus>(),
            Ok(ConfigRevisionStatus::Superseded)
        );
        assert_eq!(
            "proposed"
                .parse::<ConfigRevisionStatus>()
                .unwrap_err()
                .kind(),
            ValueKind::ConfigRevisionStatus
        );
    }

    #[test]
    fn config_revision_origin_knows_the_five_codes_of_spec_0029() {
        let codes: Vec<&str> = ConfigRevisionOrigin::ALL
            .iter()
            .map(|origin| origin.as_code())
            .collect();
        assert_eq!(
            codes,
            ["created", "applied", "endpoints", "rollback", "restored"]
        );
        assert_eq!(
            "edited".parse::<ConfigRevisionOrigin>().unwrap_err().kind(),
            ValueKind::ConfigRevisionOrigin
        );
    }

    #[test]
    fn a_revision_number_starts_at_one_and_counts_up() {
        assert_eq!(ConfigRevisionNumber::FIRST.get(), 1);
        assert_eq!(ConfigRevisionNumber::FIRST.next().get(), 2);
        assert_eq!(ConfigRevisionNumber::new(7).unwrap().to_string(), "7");
        assert_eq!(
            ConfigRevisionNumber::new(0).unwrap_err().kind(),
            ValueKind::ConfigRevisionNumber
        );
    }

    #[test]
    fn a_following_revision_takes_the_next_number_and_points_back() {
        let instance = InstanceId::generate();
        let values = ConfigValuesJson::new("{}".to_owned()).unwrap();
        let first = ConfigRevisionRecord::first(
            ConfigRevisionId::generate(),
            instance,
            values.clone(),
            EndpointRequests::default(),
            None,
            Timestamp::UNIX_EPOCH,
        );
        let next = ConfigRevisionRecord::following(
            &first,
            ConfigRevisionId::generate(),
            values,
            EndpointRequests::default(),
            ConfigRevisionOrigin::Rollback,
            Some(ConfigRevisionNumber::FIRST),
            None,
            Timestamp::UNIX_EPOCH,
        );

        assert_eq!(next.number.get(), 2);
        assert_eq!(next.previous, Some(first.id));
        assert_eq!(next.instance, instance);
        assert_eq!(next.status, ConfigRevisionStatus::Active);
        assert_eq!(next.origin, ConfigRevisionOrigin::Rollback);
        assert_eq!(next.source_number, Some(ConfigRevisionNumber::FIRST));
        assert_eq!((next.ready_at, next.rendered_sha256), (None, None));
    }

    #[test]
    fn a_first_revision_is_active_and_follows_none() {
        let id = ConfigRevisionId::generate();
        let instance = InstanceId::generate();
        let values = ConfigValuesJson::new("{}".to_owned()).unwrap();
        let record = ConfigRevisionRecord::first(
            id,
            instance,
            values.clone(),
            EndpointRequests::default(),
            None,
            Timestamp::UNIX_EPOCH,
        );

        assert_eq!(record.id, id);
        assert_eq!(record.instance, instance);
        assert_eq!(record.values, values);
        assert_eq!(record.number, ConfigRevisionNumber::FIRST);
        assert_eq!(record.status, ConfigRevisionStatus::Active);
        assert_eq!(record.origin, ConfigRevisionOrigin::Created);
        assert_eq!((record.previous, record.rendered_sha256), (None, None));
        assert_eq!((record.source_number, record.ready_at), (None, None));
        assert_eq!(record.created_by_operation, None);
        assert_eq!(record.created_at, Timestamp::UNIX_EPOCH);
    }
}
