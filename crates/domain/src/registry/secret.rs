//! What the registry keeps for secrets and audit (spec 0005, "Tambahan
//! fitur 11"): one row per secret reference, the resources that use it,
//! and the append only audit rows.
//!
//! None of these types can hold a secret value. A reference row names the
//! value through its reference text and the platform store key, and an
//! audit row holds only details the caller already redacted (BR-018,
//! AC-12).

use std::borrow::Cow;
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use jiff::Timestamp;
use uuid::Uuid;

use crate::code::code_enum;
use crate::error::{ValueError, ValueKind};
use crate::id::{EnvironmentId, InstanceId, OperationId};
use crate::registry::error::RegistryError;
use crate::registry::{AuditEventId, SecretReferenceId};
use crate::secret::{
    SecretOrigin, SecretOwner, SecretOwnerKind, SecretPurpose, SecretRef, SecretStatus,
};
use crate::value::{DetailKey, ResourceRevision};

/// Longest [`SecretFieldName`] and [`AuditAction`], in bytes.
const NAME_MAX: usize = 64;
/// Longest [`PlatformStoreKey`], in bytes.
const STORE_KEY_MAX: usize = 512;

/// The name of the field a resource uses a secret reference for: the
/// secret field name of a provider manifest (spec 0009) or a `secretRefs`
/// key of an environment. 1 to 64 bytes, a lowercase letter first, then
/// letters, digits, `_`, and `-`.
///
/// ```
/// use stackly_domain::SecretFieldName;
///
/// assert!("adminPassword".parse::<SecretFieldName>().is_ok());
/// assert!("db_url-2".parse::<SecretFieldName>().is_ok());
/// assert!("AdminPassword".parse::<SecretFieldName>().is_err());
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SecretFieldName(String);

impl SecretFieldName {
    /// Returns the text form.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SecretFieldName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for SecretFieldName {
    type Err = ValueError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bytes = s.as_bytes();
        let valid = (1..=NAME_MAX).contains(&bytes.len())
            && bytes[0].is_ascii_lowercase()
            && bytes
                .iter()
                .all(|b| b.is_ascii_alphanumeric() || *b == b'_' || *b == b'-');
        if valid {
            Ok(Self(s.to_owned()))
        } else {
            Err(ValueError::new(
                ValueKind::SecretFieldName,
                "must be 1 to 64 bytes: a lowercase letter, then letters, digits, `_`, and `-`",
            ))
        }
    }
}

/// Whether `s` is 1 to 64 bytes: a lowercase letter, then lowercase
/// letters, digits, `_`, and `.`.
const fn is_action(s: &str) -> bool {
    let bytes = s.as_bytes();
    if bytes.is_empty() || bytes.len() > NAME_MAX || !bytes[0].is_ascii_lowercase() {
        return false;
    }
    let mut index = 1;
    while index < bytes.len() {
        let b = bytes[index];
        if !(b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'.') {
            return false;
        }
        index += 1;
    }
    true
}

/// What an audit row records, such as `secret.reveal`: 1 to 64 bytes, a
/// lowercase letter first, then lowercase letters, digits, `_`, and `.`.
/// The feature that writes the row owns its actions.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AuditAction(Cow<'static, str>);

impl AuditAction {
    /// Creates an action that is fixed in the source.
    ///
    /// # Panics
    ///
    /// Panics when `action` breaks the rule. Use it in a `const` item,
    /// where that panic stops the build instead of a running program.
    #[must_use]
    pub const fn from_static(action: &'static str) -> Self {
        assert!(
            is_action(action),
            "invalid AuditAction: must be 1 to 64 bytes of [a-z][a-z0-9_.]*"
        );
        Self(Cow::Borrowed(action))
    }

    /// Returns the text form.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for AuditAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for AuditAction {
    type Err = ValueError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if is_action(s) {
            Ok(Self(Cow::Owned(s.to_owned())))
        } else {
            Err(ValueError::new(
                ValueKind::AuditAction,
                "must be 1 to 64 bytes: a lowercase letter, then lowercase letters, digits, `_`, \
                 and `.`",
            ))
        }
    }
}

/// Where the platform secret store keeps the value of a reference, as
/// `SecretStore::store_key` computes it (spec 0011), for example
/// `macos-keychain:login/stackly-secret/erp/admin`: 1 to 512 bytes. It
/// names the item and is never the value.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PlatformStoreKey(String);

impl PlatformStoreKey {
    /// Wraps a store key.
    ///
    /// # Errors
    ///
    /// Returns a [`ValueError`] for an empty key or one over 512 bytes.
    pub fn new(key: String) -> Result<Self, ValueError> {
        if (1..=STORE_KEY_MAX).contains(&key.len()) {
            Ok(Self(key))
        } else {
            Err(ValueError::new(
                ValueKind::PlatformStoreKey,
                "must be 1 to 512 bytes",
            ))
        }
    }

    /// Returns the text form.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PlatformStoreKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for PlatformStoreKey {
    type Err = ValueError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s.to_owned())
    }
}

/// A resource that uses secret references: the `resource_kind` and
/// `resource_id` of a `secret_reference_uses` row.
///
/// Uses are ordered as the registry stores them: by the kind's code (so
/// environments before instances), then by ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SecretUser {
    /// An instance, through its active config revision.
    Instance(InstanceId),
    /// An environment, through its `secretRefs`.
    Environment(EnvironmentId),
}

impl SecretUser {
    /// Returns the stored kind, `instance` or `environment`.
    #[must_use]
    pub const fn kind_code(&self) -> &'static str {
        match self {
            Self::Instance(_) => "instance",
            Self::Environment(_) => "environment",
        }
    }

    /// Returns the stored ID.
    #[must_use]
    pub const fn id(&self) -> Uuid {
        match self {
            Self::Instance(id) => id.as_uuid(),
            Self::Environment(id) => id.as_uuid(),
        }
    }

    /// Rebuilds a user from its stored kind and ID.
    ///
    /// # Errors
    ///
    /// Returns a [`ValueError`] for an unknown kind or an ID that is not a
    /// UUID version 7.
    pub fn from_parts(kind: &str, id: Uuid) -> Result<Self, ValueError> {
        let invalid = || {
            ValueError::new(
                ValueKind::SecretUser,
                "must be an instance or environment with a UUID version 7",
            )
        };
        match kind {
            "instance" => InstanceId::from_uuid(id)
                .map(Self::Instance)
                .map_err(|_| invalid()),
            "environment" => EnvironmentId::from_uuid(id)
                .map(Self::Environment)
                .map_err(|_| invalid()),
            _ => Err(invalid()),
        }
    }
}

impl PartialOrd for SecretUser {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SecretUser {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.kind_code(), self.id()).cmp(&(other.kind_code(), other.id()))
    }
}

impl fmt::Display for SecretUser {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.kind_code(), self.id().hyphenated())
    }
}

/// One use of a secret reference: a resource needs it for one field.
///
/// Ordered by reference, then user (see [`SecretUser`]), then field, the
/// order every registry read returns.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SecretUse {
    /// The reference.
    pub reference: SecretRef,
    /// The resource that uses it.
    pub user: SecretUser,
    /// The field it fills.
    pub field: SecretFieldName,
}

impl SecretUse {
    /// Describes one use.
    #[must_use]
    pub const fn new(reference: SecretRef, user: SecretUser, field: SecretFieldName) -> Self {
        Self {
            reference,
            user,
            field,
        }
    }
}

/// One `secret_references` row (spec 0011 *Model data*): where the value
/// lives and what Stackly last saw of it, never the value.
///
/// `revision` starts at 1 and rises only when `put` stores a value or a
/// failed delete restores the row; an observation of the store does not
/// raise it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct SecretReferenceRecord {
    /// The row's ID, made by the daemon.
    pub id: SecretReferenceId,
    /// The reference text, unique.
    pub reference: SecretRef,
    /// Where the platform store keeps the value.
    pub platform_store_key: PlatformStoreKey,
    /// Who owns the reference; stored as `owner_kind` and `owner_id`.
    pub owner: SecretOwner,
    /// The purpose of the field it was made for, or `user`.
    pub purpose: SecretPurpose,
    /// Where the value came from, once one was stored.
    pub value_origin: Option<SecretOrigin>,
    /// The status the store last showed.
    pub status: SecretStatus,
    /// When the status was observed.
    pub status_observed_at: Option<Timestamp>,
    /// When Stackly last stored a value.
    pub value_updated_at: Option<Timestamp>,
    /// The revision, from 1.
    pub revision: ResourceRevision,
    /// When the row was created.
    pub created_at: Timestamp,
    /// When the row was last written.
    pub updated_at: Timestamp,
}

impl SecretReferenceRecord {
    /// A new row at revision 1 for a reference that has no value yet:
    /// status `missing`, never observed, created and updated `at`. Set
    /// the value fields before inserting a row for a stored value.
    #[must_use]
    pub const fn new(
        id: SecretReferenceId,
        reference: SecretRef,
        platform_store_key: PlatformStoreKey,
        owner: SecretOwner,
        purpose: SecretPurpose,
        at: Timestamp,
    ) -> Self {
        Self {
            id,
            reference,
            platform_store_key,
            owner,
            purpose,
            value_origin: None,
            status: SecretStatus::Missing,
            status_observed_at: None,
            value_updated_at: None,
            revision: ResourceRevision::INITIAL,
            created_at: at,
            updated_at: at,
        }
    }

    /// Returns the kind of the owner, the stored `owner_kind`.
    #[must_use]
    pub const fn owner_kind(&self) -> SecretOwnerKind {
        self.owner.kind()
    }
}

/// A reference row with every use of it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct SecretReferenceView {
    /// The row.
    pub record: SecretReferenceRecord,
    /// Its uses, environments before instances, then by ID and field.
    pub used_by: Vec<SecretUse>,
}

impl SecretReferenceView {
    /// Pairs a row with its uses, which the caller orders.
    #[must_use]
    pub const fn new(record: SecretReferenceRecord, used_by: Vec<SecretUse>) -> Self {
        Self { record, used_by }
    }
}

/// Which reference rows [`Registry::secret_references`] returns, ordered
/// by reference text.
///
/// [`Registry::secret_references`]: crate::Registry::secret_references
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct SecretReferenceFilter {
    /// Only references this kind of owner owns.
    pub owner_kind: Option<SecretOwnerKind>,
    /// Only references with this cached status.
    pub status: Option<SecretStatus>,
}

impl SecretReferenceFilter {
    /// Returns whether `record` passes the filter.
    #[must_use]
    pub fn matches(&self, record: &SecretReferenceRecord) -> bool {
        self.owner_kind
            .is_none_or(|kind| kind == record.owner_kind())
            && self.status.is_none_or(|status| status == record.status)
    }
}

code_enum! {
    /// Who did what an audit row records.
    #[non_exhaustive]
    pub enum AuditActor: AuditActor {
        /// A user, through a client request.
        User => "user",
        /// The daemon, on a background trigger.
        Daemon => "daemon",
    }
}

code_enum! {
    /// How the audited action ended. A failure's error code goes into the
    /// details as `errorCode`, because wire codes hold a dot.
    pub enum AuditResult: AuditResult {
        /// It succeeded.
        Succeeded => "succeeded",
        /// It failed.
        Failed => "failed",
    }
}

/// The resource an audit row is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum AuditResource {
    /// A secret reference row, stored as `resource_kind` `secret`.
    SecretReference(SecretReferenceId),
}

impl AuditResource {
    /// Returns the stored kind, for example `secret`.
    #[must_use]
    pub const fn kind_code(&self) -> &'static str {
        match self {
            Self::SecretReference(_) => "secret",
        }
    }

    /// Returns the stored ID.
    #[must_use]
    pub const fn id(&self) -> Uuid {
        match self {
            Self::SecretReference(id) => id.as_uuid(),
        }
    }

    /// Rebuilds a resource from its stored kind and ID.
    ///
    /// # Errors
    ///
    /// Returns a [`ValueError`] for an unknown kind or an ID that is not a
    /// UUID version 7.
    pub fn from_parts(kind: &str, id: Uuid) -> Result<Self, ValueError> {
        let invalid = || {
            ValueError::new(
                ValueKind::AuditResource,
                "must be a known kind with a UUID version 7",
            )
        };
        match kind {
            "secret" => SecretReferenceId::from_uuid(id)
                .map(Self::SecretReference)
                .map_err(|_| invalid()),
            _ => Err(invalid()),
        }
    }
}

/// Why [`SafeDetails`] refused its entries.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum SafeDetailsError {
    /// More than 20 entries.
    #[error("audit details hold at most 20 entries")]
    TooMany,
    /// A value over 512 bytes.
    #[error("the audit detail {key} is longer than 512 bytes")]
    ValueTooLong {
        /// The key of the value.
        key: DetailKey,
    },
    /// The same key twice.
    #[error("the audit detail {key} is given twice")]
    DuplicateKey {
        /// The key.
        key: DetailKey,
    },
}

/// The details of an audit row: at most 20 entries of camelCase keys and
/// string values of at most 512 bytes, stored as a JSON object with its
/// keys in order.
///
/// The caller redacts every value before it builds the details, and the
/// keys belong to the caller's spec (spec 0011 *Value sourcing* for the
/// secret actions).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SafeDetails(BTreeMap<DetailKey, String>);

impl SafeDetails {
    /// The most entries.
    pub const MAX_ENTRIES: usize = 20;
    /// The longest value, in bytes.
    pub const VALUE_MAX: usize = 512;

    /// Details with no entries, stored as `{}`.
    #[must_use]
    pub const fn empty() -> Self {
        Self(BTreeMap::new())
    }

    /// Checks and holds `entries`.
    ///
    /// # Errors
    ///
    /// Returns a [`SafeDetailsError`] for more than 20 entries, a value
    /// over 512 bytes, or a key given twice.
    pub fn new(
        entries: impl IntoIterator<Item = (DetailKey, String)>,
    ) -> Result<Self, SafeDetailsError> {
        let mut details = BTreeMap::new();
        for (key, value) in entries {
            if value.len() > Self::VALUE_MAX {
                return Err(SafeDetailsError::ValueTooLong { key });
            }
            if details.contains_key(&key) {
                return Err(SafeDetailsError::DuplicateKey { key });
            }
            if details.len() == Self::MAX_ENTRIES {
                return Err(SafeDetailsError::TooMany);
            }
            details.insert(key, value);
        }
        Ok(Self(details))
    }

    /// Returns the value of `key`.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(stored, _)| stored.as_str() == key)
            .map(|(_, value)| value.as_str())
    }

    /// Returns every entry, in key order.
    pub fn iter(&self) -> impl Iterator<Item = (&DetailKey, &str)> {
        self.0.iter().map(|(key, value)| (key, value.as_str()))
    }

    /// Returns the number of entries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns whether there are no entries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// One `audit_events` row. The table is append only; rows go only through
/// prune.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct AuditEventRecord {
    /// The row's ID, made by the caller.
    pub id: AuditEventId,
    /// Who acted.
    pub actor: AuditActor,
    /// What they did.
    pub action: AuditAction,
    /// What it was about, while it is a registry row.
    pub resource: Option<AuditResource>,
    /// The operation it belongs to, if any; not checked, so history stays
    /// after the operation is pruned.
    pub operation: Option<OperationId>,
    /// How it ended.
    pub result: AuditResult,
    /// Redacted details.
    pub safe_details: SafeDetails,
    /// When it happened, from the caller's clock.
    pub occurred_at: Timestamp,
}

impl AuditEventRecord {
    /// A row with no resource, no operation, and no details.
    #[must_use]
    pub const fn new(
        id: AuditEventId,
        actor: AuditActor,
        action: AuditAction,
        result: AuditResult,
        occurred_at: Timestamp,
    ) -> Self {
        Self {
            id,
            actor,
            action,
            resource: None,
            operation: None,
            result,
            safe_details: SafeDetails::empty(),
            occurred_at,
        }
    }
}

/// Where a page of audit rows ends: the next page starts after it. Pages go
/// from the newest `(occurred_at, id)` down.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AuditCursor {
    /// When the last row of the page happened, to the nanosecond.
    pub occurred_at: Timestamp,
    /// Its ID, which breaks ties.
    pub id: AuditEventId,
}

impl AuditCursor {
    /// Returns the cursor that points at `record`.
    #[must_use]
    pub const fn of(record: &AuditEventRecord) -> Self {
        Self {
            occurred_at: record.occurred_at,
            id: record.id,
        }
    }
}

/// A page of audit rows, newest `(occurred_at, id)` first. Feature 35 adds
/// filters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AuditQuery {
    /// The most rows on the page, 1 to [`AuditQuery::LIMIT_MAX`].
    pub limit: u16,
    /// Start after this cursor, for the next page.
    pub before: Option<AuditCursor>,
}

impl AuditQuery {
    /// The largest page.
    pub const LIMIT_MAX: u16 = 500;

    /// Checks the page limit.
    ///
    /// # Errors
    ///
    /// Returns [`RegistryError::InvalidQuery`] when `limit` is outside 1 to
    /// [`AuditQuery::LIMIT_MAX`].
    pub const fn validate(&self) -> Result<(), RegistryError> {
        if self.limit == 0 || self.limit > Self::LIMIT_MAX {
            return Err(RegistryError::InvalidQuery {
                reason: "the page limit must be 1 to 500",
            });
        }
        Ok(())
    }
}

/// One page of audit rows, and the cursor of the next page while rows
/// remain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditPage {
    /// The rows, newest `(occurred_at, id)` first.
    pub items: Vec<AuditEventRecord>,
    /// Where the next page starts, or `None` on the last page.
    pub next: Option<AuditCursor>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(text: &str) -> DetailKey {
        text.parse().unwrap()
    }

    #[test]
    fn field_names_take_a_lowercase_letter_then_letters_digits_underscores_and_hyphens() {
        for good in ["a", "adminPassword", "db_url", "tls-key-2", &"a".repeat(64)] {
            assert!(good.parse::<SecretFieldName>().is_ok(), "{good:?}");
        }
        for bad in ["", "Admin", "1a", "a.b", "a b", &"a".repeat(65)] {
            assert!(bad.parse::<SecretFieldName>().is_err(), "{bad:?}");
        }
    }

    #[test]
    fn audit_actions_take_lowercase_codes_joined_by_dots() {
        assert_eq!(
            AuditAction::from_static("secret.reveal").as_str(),
            "secret.reveal"
        );
        for good in ["secret.set", "test.seed", "a", "a_b.c2"] {
            assert!(good.parse::<AuditAction>().is_ok(), "{good:?}");
        }
        for bad in ["", "Secret.set", ".set", "secret-set", &"a".repeat(65)] {
            assert!(bad.parse::<AuditAction>().is_err(), "{bad:?}");
        }
    }

    #[test]
    fn store_keys_are_1_to_512_bytes() {
        assert!(PlatformStoreKey::new("fake:stackly-secret/a/b".to_owned()).is_ok());
        assert!(PlatformStoreKey::new("x".repeat(512)).is_ok());
        assert!(PlatformStoreKey::new(String::new()).is_err());
        assert!(PlatformStoreKey::new("x".repeat(513)).is_err());
    }

    #[test]
    fn users_order_environments_before_instances_and_round_trip_their_parts() {
        let instance = SecretUser::Instance(InstanceId::generate());
        let environment = SecretUser::Environment(EnvironmentId::generate());

        assert!(environment < instance);
        for user in [instance, environment] {
            assert_eq!(
                SecretUser::from_parts(user.kind_code(), user.id()).unwrap(),
                user
            );
        }
        assert!(SecretUser::from_parts("runtime", InstanceId::generate().as_uuid()).is_err());
        assert!(SecretUser::from_parts("instance", Uuid::nil()).is_err());
    }

    #[test]
    fn audit_resources_round_trip_their_parts() {
        let resource = AuditResource::SecretReference(SecretReferenceId::generate());

        assert_eq!(resource.kind_code(), "secret");
        assert_eq!(
            AuditResource::from_parts("secret", resource.id()).unwrap(),
            resource
        );
        assert!(AuditResource::from_parts("instance", resource.id()).is_err());
    }

    #[test]
    fn safe_details_hold_at_most_20_short_values_with_unique_keys() {
        let details = SafeDetails::new([(key("reference"), "local:a/b".to_owned())]).unwrap();
        assert_eq!(details.get("reference"), Some("local:a/b"));
        assert_eq!(details.len(), 1);
        assert!(SafeDetails::empty().is_empty());

        let twenty = (0..20).map(|n| (key(&format!("k{n}")), String::new()));
        assert!(SafeDetails::new(twenty).is_ok());
        let many = (0..21).map(|n| (key(&format!("k{n}")), String::new()));
        assert_eq!(SafeDetails::new(many), Err(SafeDetailsError::TooMany));
        assert_eq!(
            SafeDetails::new([(key("a"), "x".repeat(513))]),
            Err(SafeDetailsError::ValueTooLong { key: key("a") })
        );
        assert_eq!(
            SafeDetails::new([(key("a"), String::new()), (key("a"), String::new())]),
            Err(SafeDetailsError::DuplicateKey { key: key("a") })
        );
    }

    #[test]
    fn a_filter_matches_on_owner_kind_and_status() {
        let mut record = SecretReferenceRecord::new(
            SecretReferenceId::generate(),
            SecretRef::parse("local:erp/admin").unwrap(),
            PlatformStoreKey::new("fake:stackly-secret/erp/admin".to_owned()).unwrap(),
            SecretOwner::User,
            SecretPurpose::user(),
            Timestamp::UNIX_EPOCH,
        );
        record.status = SecretStatus::Present;

        assert!(SecretReferenceFilter::default().matches(&record));
        let user_present = SecretReferenceFilter {
            owner_kind: Some(SecretOwnerKind::User),
            status: Some(SecretStatus::Present),
        };
        assert!(user_present.matches(&record));
        let instance = SecretReferenceFilter {
            owner_kind: Some(SecretOwnerKind::Instance),
            status: None,
        };
        assert!(!instance.matches(&record));
    }

    #[test]
    fn an_audit_query_takes_1_to_500_rows() {
        assert!(
            AuditQuery {
                limit: 1,
                before: None
            }
            .validate()
            .is_ok()
        );
        assert!(
            AuditQuery {
                limit: 500,
                before: None
            }
            .validate()
            .is_ok()
        );
        assert!(
            AuditQuery {
                limit: 0,
                before: None
            }
            .validate()
            .is_err()
        );
        assert!(
            AuditQuery {
                limit: 501,
                before: None
            }
            .validate()
            .is_err()
        );
    }
}
