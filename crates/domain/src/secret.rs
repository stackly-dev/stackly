//! Secret references and the facts the daemon keeps about them (spec
//! 0011): the `local:<path>` reference grammar, the purpose a secret field
//! name derives, who owns a reference, its last observed status, where its
//! value came from, and the blocker an instance gets when a secret cannot
//! be read.
//!
//! None of these types ever holds a secret value. A reference is not a
//! secret: it may appear in logs, the GUI, the CLI, diagnostics, and audit.

use std::fmt;
use std::str::FromStr;

use jiff::Timestamp;
use uuid::Uuid;

use crate::code::code_enum;
use crate::error::{ValueError, ValueKind};
use crate::id::{EnvironmentId, InstanceId};
use crate::process::IdentityEvidence;
use crate::text::serde_as_text;

/// The longest secret reference, in bytes, the `local:` prefix included.
const REFERENCE_MAX: usize = 255;
/// The longest segment of a reference, and the longest purpose, in bytes.
const SEGMENT_MAX: usize = 64;
/// The fewest segments a reference holds.
const SEGMENTS_MIN: usize = 2;
/// The most segments a reference holds.
const SEGMENTS_MAX: usize = 8;
/// The first segment of the references only the daemon creates.
const INSTANCE_SEGMENT: &str = "instance";
/// The first segment reserved for Stackly's own secrets.
const RESERVED_SEGMENT: &str = "stackly";

/// Why a text is not a [`SecretRef`]. The message names the broken rule
/// and never the text itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, thiserror::Error)]
#[non_exhaustive]
pub enum SecretRefError {
    /// The text is longer than 255 bytes.
    #[error("invalid secret reference: must be at most 255 bytes")]
    TooLong,
    /// The text has no scheme.
    #[error("invalid secret reference: must start with `local:`")]
    MissingScheme,
    /// The scheme is not `local`, for example `keychain:`. Other schemes
    /// are reserved for other secret providers.
    #[error("invalid secret reference: unsupported scheme")]
    UnsupportedScheme,
    /// The path does not hold 2 to 8 segments.
    #[error("invalid secret reference: must hold 2 to 8 segments separated by `/`")]
    SegmentCount,
    /// A segment is empty, as in `local:a//b` or `local:a/`.
    #[error("invalid secret reference: a segment must not be empty")]
    EmptySegment,
    /// A segment is `.` or `..`.
    #[error("invalid secret reference: a segment must not be `.` or `..`")]
    DotSegment,
    /// A segment is longer than 64 bytes.
    #[error("invalid secret reference: a segment must be at most 64 bytes")]
    SegmentTooLong,
    /// A segment breaks the segment pattern.
    #[error("invalid secret reference: a segment must match `[a-z0-9][a-z0-9._-]*`")]
    InvalidSegment,
}

impl SecretRefError {
    /// Returns the broken rule alone, for example `unsupported scheme`,
    /// for a structured error detail.
    #[must_use]
    pub const fn reason(self) -> &'static str {
        match self {
            Self::TooLong => "must be at most 255 bytes",
            Self::MissingScheme => "must start with `local:`",
            Self::UnsupportedScheme => "unsupported scheme",
            Self::SegmentCount => "must hold 2 to 8 segments separated by `/`",
            Self::EmptySegment => "a segment must not be empty",
            Self::DotSegment => "a segment must not be `.` or `..`",
            Self::SegmentTooLong => "a segment must be at most 64 bytes",
            Self::InvalidSegment => "a segment must match `[a-z0-9][a-z0-9._-]*`",
        }
    }
}

/// Checks one segment: 1 to 64 bytes matching `[a-z0-9][a-z0-9._-]*`, and
/// not `.` or `..`.
fn check_segment(segment: &str) -> Result<(), SecretRefError> {
    let bytes = segment.as_bytes();
    if bytes.is_empty() {
        return Err(SecretRefError::EmptySegment);
    }
    if segment == "." || segment == ".." {
        return Err(SecretRefError::DotSegment);
    }
    if bytes.len() > SEGMENT_MAX {
        return Err(SecretRefError::SegmentTooLong);
    }
    let lower_or_digit = |b: &u8| b.is_ascii_lowercase() || b.is_ascii_digit();
    if bytes.first().is_some_and(lower_or_digit)
        && bytes
            .iter()
            .all(|b| lower_or_digit(b) || matches!(b, b'.' | b'_' | b'-'))
    {
        Ok(())
    } else {
        Err(SecretRefError::InvalidSegment)
    }
}

/// A reference to one secret in the user's secret store, in the form
/// `local:<segment>/<segment>...` (AC-1).
///
/// `local` means the secret store of this user on this Mac: every person
/// fills the value in locally, so the same `stackly.yaml` works on another
/// Mac. The path holds 2 to 8 segments, each 1 to 64 bytes matching
/// `[a-z0-9][a-z0-9._-]*` and never `.` or `..`, and the whole text is at
/// most 255 bytes. Any other scheme, `keychain:` included, is rejected.
/// A parsed reference prints back to exactly the text it came from, and
/// orders by that text.
///
/// ```
/// use stackly_domain::{SecretRef, SecretRefError};
///
/// let reference = SecretRef::parse("local:erp/postgres/admin")?;
/// assert_eq!(reference.to_string(), "local:erp/postgres/admin");
/// assert_eq!(reference.path(), "erp/postgres/admin");
/// assert_eq!(
///     SecretRef::parse("keychain:erp/admin"),
///     Err(SecretRefError::UnsupportedScheme)
/// );
/// # Ok::<(), SecretRefError>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SecretRef(String);

impl SecretRef {
    /// The only scheme this version accepts.
    pub const SCHEME: &'static str = "local";

    /// Parses a reference.
    ///
    /// # Errors
    ///
    /// Returns the [`SecretRefError`] that names the first rule the text
    /// breaks: [`SecretRefError::UnsupportedScheme`] for any scheme but
    /// `local`, and a segment or length rule otherwise.
    pub fn parse(text: &str) -> Result<Self, SecretRefError> {
        if text.len() > REFERENCE_MAX {
            return Err(SecretRefError::TooLong);
        }
        let Some((scheme, path)) = text.split_once(':') else {
            return Err(SecretRefError::MissingScheme);
        };
        if scheme.is_empty() {
            return Err(SecretRefError::MissingScheme);
        }
        if scheme != Self::SCHEME {
            return Err(SecretRefError::UnsupportedScheme);
        }
        let count = path.split('/').count();
        if !(SEGMENTS_MIN..=SEGMENTS_MAX).contains(&count) {
            return Err(SecretRefError::SegmentCount);
        }
        path.split('/').try_for_each(check_segment)?;
        Ok(Self(text.to_owned()))
    }

    /// Returns the reference the daemon creates for one secret field of an
    /// instance, `local:instance/<instance-id>/<purpose>`, with the
    /// instance ID in lowercase.
    #[must_use]
    pub fn for_instance(instance: InstanceId, purpose: &SecretPurpose) -> Self {
        Self(format!(
            "{}:{INSTANCE_SEGMENT}/{instance}/{purpose}",
            Self::SCHEME
        ))
    }

    /// Returns the whole text, `local:` included.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns the path without `local:`, for example `erp/postgres/admin`.
    /// The macOS store uses it as the item's account.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.0[Self::SCHEME.len() + 1..]
    }

    /// Returns the segments of the path, in order.
    pub fn segments(&self) -> impl Iterator<Item = &str> {
        self.path().split('/')
    }

    /// Returns the first segment of the path.
    #[must_use]
    pub fn first_segment(&self) -> &str {
        self.path().split('/').next().unwrap_or_default()
    }

    /// Returns true when the first segment is `instance`: only the daemon
    /// creates such a reference, for a secret field of an instance.
    #[must_use]
    pub fn is_instance_reference(&self) -> bool {
        self.first_segment() == INSTANCE_SEGMENT
    }

    /// Returns true when the first segment is `stackly`, reserved for
    /// Stackly's own secrets.
    #[must_use]
    pub fn is_reserved(&self) -> bool {
        self.first_segment() == RESERVED_SEGMENT
    }
}

impl fmt::Display for SecretRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for SecretRef {
    type Err = SecretRefError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

serde_as_text!(SecretRef => "a secret reference such as local:erp/postgres/admin");

/// What a secret is for: the last segment of an instance reference, the
/// purpose of a registry row, at most 64 bytes matching the segment
/// pattern `[a-z0-9][a-z0-9._-]*`.
///
/// [`from_field_name`](Self::from_field_name) derives it from the name of
/// a secret field (AC-6), and [`SecretPurpose::user`] is the purpose of a
/// reference a user registered through `secret.set`.
///
/// ```
/// use stackly_domain::SecretPurpose;
///
/// let purpose = SecretPurpose::from_field_name("adminPassword")?;
/// assert_eq!(purpose.as_str(), "admin-password");
/// # Ok::<(), stackly_domain::ValueError>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SecretPurpose(String);

impl SecretPurpose {
    /// The purpose of a reference a user registered through `secret.set`.
    #[must_use]
    pub fn user() -> Self {
        Self("user".to_owned())
    }

    /// Derives the purpose of a secret field from its name, which matches
    /// `[a-z][A-Za-z0-9]*` (spec 0009): a `-` goes before every uppercase
    /// letter that follows a lowercase letter or a digit, then every
    /// letter is lowercased. `adminPassword` becomes `admin-password`,
    /// `tlsKeyPEM` becomes `tls-key-pem`, and `token2Fa` becomes
    /// `token2-fa`.
    ///
    /// # Errors
    ///
    /// Returns a [`ValueError`] when the name breaks the field name
    /// pattern, or when the purpose would be longer than 64 bytes.
    pub fn from_field_name(name: &str) -> Result<Self, ValueError> {
        let bytes = name.as_bytes();
        if !bytes.first().is_some_and(u8::is_ascii_lowercase)
            || !bytes.iter().all(u8::is_ascii_alphanumeric)
        {
            return Err(ValueError::new(
                ValueKind::SecretPurpose,
                "a secret field name must match `[a-z][A-Za-z0-9]*`",
            ));
        }
        let mut purpose = String::with_capacity(name.len() + 4);
        let mut previous: Option<u8> = None;
        for &byte in bytes {
            let after_lower_or_digit =
                previous.is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit());
            if byte.is_ascii_uppercase() && after_lower_or_digit {
                purpose.push('-');
            }
            purpose.push(char::from(byte.to_ascii_lowercase()));
            previous = Some(byte);
        }
        if purpose.len() > SEGMENT_MAX {
            return Err(ValueError::new(
                ValueKind::SecretPurpose,
                "must be at most 64 bytes once derived from the field name",
            ));
        }
        Ok(Self(purpose))
    }

    /// Returns the text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SecretPurpose {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for SecretPurpose {
    type Err = ValueError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        check_segment(s).map_err(|_| {
            ValueError::new(
                ValueKind::SecretPurpose,
                "must be 1 to 64 bytes matching `[a-z0-9][a-z0-9._-]*`, not `.` or `..`",
            )
        })?;
        Ok(Self(s.to_owned()))
    }
}

serde_as_text!(SecretPurpose => "a secret purpose such as admin-password");

code_enum! {
    /// The kind of resource that owns a secret reference.
    pub enum SecretOwnerKind: SecretOwnerKind {
        /// An instance owns it; the daemon created it for a secret field.
        Instance => "instance",
        /// An environment owns it, through its `secretRefs`.
        Environment => "environment",
        /// The user owns it: registered through `secret.set`, or left over
        /// when its owner was removed while another resource used it.
        User => "user",
    }
}

/// Who owns a secret reference. It is set when the registry row is born
/// and never changes, except that it becomes [`SecretOwner::User`] when
/// its owner is removed while another resource still uses the reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SecretOwner {
    /// An instance.
    Instance(InstanceId),
    /// An environment.
    Environment(EnvironmentId),
    /// The user.
    User,
}

impl SecretOwner {
    /// Rebuilds an owner from its registry columns, `owner_kind` and
    /// `owner_id`.
    ///
    /// # Errors
    ///
    /// Returns a [`ValueError`] when an instance or environment owner has
    /// no ID or an ID that is not a UUID version 7, or a user owner has an
    /// ID.
    pub fn from_parts(kind: SecretOwnerKind, id: Option<Uuid>) -> Result<Self, ValueError> {
        let needs_id = || {
            ValueError::new(
                ValueKind::SecretOwner,
                "an instance or environment owner needs a UUID version 7",
            )
        };
        match (kind, id) {
            (SecretOwnerKind::Instance, Some(id)) => InstanceId::from_uuid(id)
                .map(Self::Instance)
                .map_err(|_| needs_id()),
            (SecretOwnerKind::Environment, Some(id)) => EnvironmentId::from_uuid(id)
                .map(Self::Environment)
                .map_err(|_| needs_id()),
            (SecretOwnerKind::User, None) => Ok(Self::User),
            (SecretOwnerKind::User, Some(_)) => Err(ValueError::new(
                ValueKind::SecretOwner,
                "a user owner has no ID",
            )),
            (SecretOwnerKind::Instance | SecretOwnerKind::Environment, None) => Err(needs_id()),
        }
    }

    /// Returns the kind, the registry's `owner_kind`.
    #[must_use]
    pub const fn kind(&self) -> SecretOwnerKind {
        match self {
            Self::Instance(_) => SecretOwnerKind::Instance,
            Self::Environment(_) => SecretOwnerKind::Environment,
            Self::User => SecretOwnerKind::User,
        }
    }

    /// Returns the owner's ID, the registry's `owner_id`, or `None` for
    /// the user.
    #[must_use]
    pub const fn id(&self) -> Option<Uuid> {
        match self {
            Self::Instance(id) => Some(id.as_uuid()),
            Self::Environment(id) => Some(id.as_uuid()),
            Self::User => None,
        }
    }
}

code_enum! {
    /// The status of a reference as the secret store last showed it. The
    /// registry caches it; only reading the store proves a value is there.
    pub enum SecretStatus: SecretStatus {
        /// The store holds a value for it.
        Present => "present",
        /// The store holds no value for it.
        Missing => "missing",
        /// The store could not be checked: locked, unsupported, busy, or a
        /// backend error.
        Unknown => "unknown",
    }
}

code_enum! {
    /// Where the value behind a reference came from, the last time Stackly
    /// stored it.
    pub enum SecretOrigin: SecretOrigin {
        /// The daemon generated it.
        Generated => "generated",
        /// A client sent it.
        Entered => "entered",
        /// It was copied from another reference.
        Copied => "copied",
    }
}

code_enum! {
    /// Why a secret a resource needs could not be read (AC-11).
    pub enum NeedsSecretReason: NeedsSecretReason {
        /// The store holds no value for the reference.
        Missing => "missing",
        /// The store is locked.
        StoreLocked => "store_locked",
        /// The store refused access: the user denied a dialog, or the
        /// item does not trust this process.
        AccessDenied => "access_denied",
        /// The store did not answer in time.
        StoreBusy => "store_busy",
        /// The store is not supported on this OS.
        StoreUnsupported => "store_unsupported",
    }
}

/// One reference a resource needs but could not read.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct NeedsSecretItem {
    /// The reference.
    pub reference: SecretRef,
    /// Why it could not be read.
    pub reason: NeedsSecretReason,
    /// False when a generated value cannot work, because a use of the
    /// reference is a field that cannot change once the data is
    /// initialized and the data is.
    pub generate_allowed: bool,
}

impl NeedsSecretItem {
    /// Bundles one reference with why it could not be read.
    #[must_use]
    pub const fn new(
        reference: SecretRef,
        reason: NeedsSecretReason,
        generate_allowed: bool,
    ) -> Self {
        Self {
            reference,
            reason,
            generate_allowed,
        }
    }
}

/// Why an instance cannot start or be acted on right now: an observation,
/// like readiness and health, that never changes the lifecycle, the
/// desired state, or the revision, and lives only in the daemon's memory.
///
/// An instance holds at most one blocker. When several apply, the
/// supervisor shows `identityUnproven`, then `dataInUse`, then
/// `needsSecret` (spec 0010, *Status dan event*).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum InstanceBlocker {
    /// A secret the instance needs could not be read (AC-11).
    NeedsSecret {
        /// Every reference that could not be read, ordered by its text.
        references: Vec<NeedsSecretItem>,
        /// When the failed resolve happened.
        observed_at: Timestamp,
    },
    /// A process Stackly cannot prove is the instance's own holds the
    /// instance lock, so nothing may start on the data (spec 0010).
    DataInUse {
        /// What gathering evidence found; checks that did not run are
        /// `not_checked`.
        evidence: IdentityEvidence,
        /// When the evidence was gathered.
        observed_at: Timestamp,
    },
    /// A process matches the recorded identity but does not hold the
    /// instance lock, or gathering the evidence failed, so the supervisor
    /// neither adopts nor signals it (spec 0010).
    IdentityUnproven {
        /// What gathering evidence found, with the error if one stopped it.
        evidence: IdentityEvidence,
        /// When the evidence was gathered.
        observed_at: Timestamp,
    },
}

impl InstanceBlocker {
    /// Builds a needs secret blocker, with the references ordered by their
    /// text and one item per reference (the first one given wins).
    #[must_use]
    pub fn needs_secret(
        references: impl IntoIterator<Item = NeedsSecretItem>,
        observed_at: Timestamp,
    ) -> Self {
        let mut references: Vec<_> = references.into_iter().collect();
        references.sort_by(|a, b| a.reference.cmp(&b.reference));
        references.dedup_by(|later, kept| later.reference == kept.reference);
        Self::NeedsSecret {
            references,
            observed_at,
        }
    }

    /// Builds a data in use blocker from the gathered evidence.
    #[must_use]
    pub const fn data_in_use(evidence: IdentityEvidence, observed_at: Timestamp) -> Self {
        Self::DataInUse {
            evidence,
            observed_at,
        }
    }

    /// Builds an identity unproven blocker from the gathered evidence.
    #[must_use]
    pub const fn identity_unproven(evidence: IdentityEvidence, observed_at: Timestamp) -> Self {
        Self::IdentityUnproven {
            evidence,
            observed_at,
        }
    }

    /// Returns the stable snake_case code, for example `needs_secret`.
    #[must_use]
    pub const fn as_code(&self) -> &'static str {
        match self {
            Self::NeedsSecret { .. } => "needs_secret",
            Self::DataInUse { .. } => "data_in_use",
            Self::IdentityUnproven { .. } => "identity_unproven",
        }
    }

    /// Returns when the blocker was observed.
    #[must_use]
    pub const fn observed_at(&self) -> Timestamp {
        match self {
            Self::NeedsSecret { observed_at, .. }
            | Self::DataInUse { observed_at, .. }
            | Self::IdentityUnproven { observed_at, .. } => *observed_at,
        }
    }

    /// Returns the identity evidence of a `data_in_use` or
    /// `identity_unproven` blocker, and `None` for the others.
    #[must_use]
    pub const fn evidence(&self) -> Option<&IdentityEvidence> {
        match self {
            Self::DataInUse { evidence, .. } | Self::IdentityUnproven { evidence, .. } => {
                Some(evidence)
            }
            Self::NeedsSecret { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use stackly_evidence::{Kind, evidence};

    use super::*;

    #[test]
    fn a_reference_with_two_to_eight_valid_segments_parses_and_prints_back() {
        let ev = evidence!(
            covers = ["REQ-016", "BR-018"],
            kind = Kind::Fixture,
            expected = "a local: reference with 2 to 8 segments, each 1 to 64 bytes of [a-z0-9][a-z0-9._-]*, at most 255 bytes in all, parses and prints back to exactly the same text",
        );
        let longest_segment = "a".repeat(64);
        let eight = "local:a/b/c/d/e/f/g/h".to_owned();
        let longest = format!(
            "local:{}/{}/{}/{}",
            "a".repeat(64),
            "b".repeat(64),
            "c".repeat(64),
            "d".repeat(54)
        );
        assert_eq!(longest.len(), 255);
        for text in [
            "local:erp/postgres/admin",
            "local:a/b",
            "local:0/9",
            "local:instance/01928c3e-6f1a-7b2c-8d4e-5f60718293a4/admin-password",
            "local:my.app/db_main/pass-1",
            &format!("local:x/{longest_segment}"),
            &eight,
            &longest,
        ] {
            let reference = SecretRef::parse(text).unwrap_or_else(|err| panic!("{text}: {err}"));
            assert_eq!(reference.to_string(), text);
            assert_eq!(reference.as_str(), text);
            assert_eq!(text.parse::<SecretRef>().unwrap(), reference);
        }
        ev.pass();
    }

    #[test]
    fn a_reference_that_breaks_a_rule_names_the_rule() {
        let ev = evidence!(
            covers = ["REQ-016", "BR-018"],
            kind = Kind::Fixture,
            expected = "every text that is not a valid local: reference is rejected with the rule it breaks, and keychain: or any other scheme is rejected as unsupported scheme",
        );
        let long_segment = format!("local:x/{}", "a".repeat(65));
        let too_long = format!(
            "local:{}/{}/{}/{}",
            "a".repeat(64),
            "b".repeat(64),
            "c".repeat(64),
            "d".repeat(55)
        );
        assert_eq!(too_long.len(), 256);
        let cases = [
            ("keychain:erp/admin", SecretRefError::UnsupportedScheme),
            ("Local:erp/admin", SecretRefError::UnsupportedScheme),
            ("secret://local/erp", SecretRefError::UnsupportedScheme),
            ("erp/admin", SecretRefError::MissingScheme),
            (":erp/admin", SecretRefError::MissingScheme),
            ("", SecretRefError::MissingScheme),
            ("local:erp", SecretRefError::SegmentCount),
            ("local:", SecretRefError::SegmentCount),
            ("local:a/b/c/d/e/f/g/h/i", SecretRefError::SegmentCount),
            ("local:erp//admin", SecretRefError::EmptySegment),
            ("local:erp/admin/", SecretRefError::EmptySegment),
            ("local:/erp/admin", SecretRefError::EmptySegment),
            ("local:erp/../admin", SecretRefError::DotSegment),
            ("local:erp/.", SecretRefError::DotSegment),
            ("local:Erp/admin", SecretRefError::InvalidSegment),
            ("local:erp/ad min", SecretRefError::InvalidSegment),
            ("local:erp/-admin", SecretRefError::InvalidSegment),
            ("local:erp/.hidden", SecretRefError::InvalidSegment),
            ("local:erp/admin:x", SecretRefError::InvalidSegment),
            ("local:erp/ádmin", SecretRefError::InvalidSegment),
            (long_segment.as_str(), SecretRefError::SegmentTooLong),
            (too_long.as_str(), SecretRefError::TooLong),
        ];
        for (text, expected) in cases {
            assert_eq!(SecretRef::parse(text), Err(expected), "{text:?}");
        }
        assert_eq!(
            SecretRefError::UnsupportedScheme.to_string(),
            "invalid secret reference: unsupported scheme"
        );
        assert_eq!(
            SecretRefError::UnsupportedScheme.reason(),
            "unsupported scheme"
        );
        ev.pass();
    }

    #[test]
    fn a_reference_exposes_its_path_segments_and_reserved_first_segments() {
        let user = SecretRef::parse("local:erp/postgres/admin").unwrap();
        let instance = SecretRef::parse("local:instance/abc/admin-password").unwrap();
        let reserved = SecretRef::parse("local:stackly/internal").unwrap();

        assert_eq!(user.path(), "erp/postgres/admin");
        assert_eq!(
            user.segments().collect::<Vec<_>>(),
            ["erp", "postgres", "admin"]
        );
        assert_eq!(user.first_segment(), "erp");
        assert!(!user.is_instance_reference() && !user.is_reserved());
        assert!(instance.is_instance_reference() && !instance.is_reserved());
        assert!(reserved.is_reserved() && !reserved.is_instance_reference());
    }

    #[test]
    fn an_instance_reference_holds_the_lowercase_id_and_the_purpose() {
        let id: InstanceId = "01928C3E-6F1A-7B2C-8D4E-5F60718293A4".parse().unwrap();
        let purpose = SecretPurpose::from_field_name("adminPassword").unwrap();

        let reference = SecretRef::for_instance(id, &purpose);

        assert_eq!(
            reference.as_str(),
            "local:instance/01928c3e-6f1a-7b2c-8d4e-5f60718293a4/admin-password"
        );
        assert_eq!(SecretRef::parse(reference.as_str()).unwrap(), reference);
        assert!(reference.is_instance_reference());
    }

    #[test]
    fn references_order_by_their_text() {
        let mut references: Vec<SecretRef> = ["local:b/a", "local:a/b", "local:a/a"]
            .iter()
            .map(|text| SecretRef::parse(text).unwrap())
            .collect();
        references.sort();

        let texts: Vec<_> = references.iter().map(SecretRef::as_str).collect();
        assert_eq!(texts, ["local:a/a", "local:a/b", "local:b/a"]);
    }

    #[test]
    fn a_reference_travels_through_serde_as_its_text() {
        let reference = SecretRef::parse("local:erp/admin").unwrap();

        let json = serde_json::to_string(&reference).unwrap();
        assert_eq!(json, r#""local:erp/admin""#);
        assert_eq!(serde_json::from_str::<SecretRef>(&json).unwrap(), reference);
        assert!(serde_json::from_str::<SecretRef>(r#""keychain:erp/admin""#).is_err());
    }

    #[test]
    fn the_purpose_of_a_field_name_follows_the_spec_table() {
        let cases = [
            ("adminPassword", "admin-password"),
            ("tlsKeyPEM", "tls-key-pem"),
            ("token2Fa", "token2-fa"),
            ("password", "password"),
            ("a", "a"),
            ("aBC", "a-bc"),
            ("x1Y2Z", "x1-y2-z"),
            ("apiKey", "api-key"),
        ];
        for (field, expected) in cases {
            let purpose = SecretPurpose::from_field_name(field).unwrap();
            assert_eq!(purpose.as_str(), expected, "{field}");
            assert_eq!(expected.parse::<SecretPurpose>().unwrap(), purpose);
        }
    }

    #[test]
    fn a_field_name_outside_its_pattern_or_too_long_has_no_purpose() {
        let longest = format!("a{}", "b".repeat(63));
        assert_eq!(
            SecretPurpose::from_field_name(&longest).unwrap().as_str(),
            longest
        );
        let hyphens_push_it_over = format!("a{}", "Bc".repeat(31));
        for field in [
            "",
            "Admin",
            "1admin",
            "admin_password",
            "admin-password",
            "admín",
            format!("a{}", "b".repeat(64)).as_str(),
            hyphens_push_it_over.as_str(),
        ] {
            let err = SecretPurpose::from_field_name(field).unwrap_err();
            assert_eq!(err.kind(), ValueKind::SecretPurpose, "{field:?}");
        }
    }

    #[test]
    fn a_stored_purpose_is_one_reference_segment() {
        assert_eq!(SecretPurpose::user().as_str(), "user");
        assert_eq!(
            "user".parse::<SecretPurpose>().unwrap(),
            SecretPurpose::user()
        );
        for text in ["", ".", "..", "Admin", "-a", &"a".repeat(65)] {
            let err = text.parse::<SecretPurpose>().unwrap_err();
            assert_eq!(err.kind(), ValueKind::SecretPurpose, "{text:?}");
        }
    }

    #[test]
    fn an_owner_round_trips_through_its_registry_columns() {
        let instance = InstanceId::generate();
        let environment = EnvironmentId::generate();
        for owner in [
            SecretOwner::Instance(instance),
            SecretOwner::Environment(environment),
            SecretOwner::User,
        ] {
            assert_eq!(
                SecretOwner::from_parts(owner.kind(), owner.id()).unwrap(),
                owner
            );
        }
        assert_eq!(SecretOwner::User.kind().as_code(), "user");
        assert_eq!(
            SecretOwner::Instance(instance).id(),
            Some(instance.as_uuid())
        );
    }

    #[test]
    fn an_owner_with_a_missing_or_extra_id_is_rejected() {
        let v4 = Uuid::from_u128(0x0192_8c3e_6f1a_4b2c_8d4e_5f60_7182_93a4);
        let v7 = InstanceId::generate().as_uuid();
        for (kind, id) in [
            (SecretOwnerKind::Instance, None),
            (SecretOwnerKind::Environment, None),
            (SecretOwnerKind::Instance, Some(v4)),
            (SecretOwnerKind::User, Some(v7)),
        ] {
            let err = SecretOwner::from_parts(kind, id).unwrap_err();
            assert_eq!(err.kind(), ValueKind::SecretOwner, "{kind} {id:?}");
        }
    }

    #[test]
    fn status_origin_reason_and_owner_kind_codes_are_stable() {
        let codes = |all: &[&'static str]| all.join(",");
        assert_eq!(
            codes(
                &SecretStatus::ALL
                    .iter()
                    .map(|s| s.as_code())
                    .collect::<Vec<_>>()
            ),
            "present,missing,unknown"
        );
        assert_eq!(
            codes(
                &SecretOrigin::ALL
                    .iter()
                    .map(|s| s.as_code())
                    .collect::<Vec<_>>()
            ),
            "generated,entered,copied"
        );
        assert_eq!(
            codes(
                &NeedsSecretReason::ALL
                    .iter()
                    .map(|s| s.as_code())
                    .collect::<Vec<_>>()
            ),
            "missing,store_locked,access_denied,store_busy,store_unsupported"
        );
        assert_eq!(
            codes(
                &SecretOwnerKind::ALL
                    .iter()
                    .map(|s| s.as_code())
                    .collect::<Vec<_>>()
            ),
            "instance,environment,user"
        );
        assert_eq!(
            "store_locked".parse::<NeedsSecretReason>().unwrap(),
            NeedsSecretReason::StoreLocked
        );
        assert_eq!(
            "locked".parse::<NeedsSecretReason>().unwrap_err().kind(),
            ValueKind::NeedsSecretReason
        );
    }

    #[test]
    fn a_needs_secret_blocker_orders_its_references_and_keeps_one_per_reference() {
        let at: Timestamp = "2026-01-01T00:00:00Z".parse().unwrap();
        let item = |text: &str, reason| {
            NeedsSecretItem::new(SecretRef::parse(text).unwrap(), reason, true)
        };

        let blocker = InstanceBlocker::needs_secret(
            [
                item("local:b/x", NeedsSecretReason::Missing),
                item("local:a/x", NeedsSecretReason::StoreLocked),
                item("local:b/x", NeedsSecretReason::AccessDenied),
            ],
            at,
        );

        let InstanceBlocker::NeedsSecret {
            references,
            observed_at,
        } = &blocker
        else {
            panic!("needs_secret builds a NeedsSecret blocker: {blocker:?}");
        };
        let summary: Vec<_> = references
            .iter()
            .map(|item| (item.reference.as_str(), item.reason))
            .collect();
        assert_eq!(
            summary,
            [
                ("local:a/x", NeedsSecretReason::StoreLocked),
                ("local:b/x", NeedsSecretReason::Missing),
            ]
        );
        assert_eq!(*observed_at, at);
        assert_eq!(blocker.observed_at(), at);
        assert_eq!(blocker.as_code(), "needs_secret");
        assert_eq!(blocker.evidence(), None);
    }

    #[test]
    fn identity_blockers_carry_their_evidence_and_observation_time() {
        use crate::process::{IdentityCheck, IdentityCheckName, LockState, Pid};

        let at: Timestamp = "2026-10-19T08:00:00Z".parse().unwrap();
        let evidence = IdentityEvidence::new()
            .with_check(IdentityCheckName::BootMatches, IdentityCheck::Failed)
            .with_pid(Pid::new(4242).unwrap())
            .with_lock(LockState::Held);

        let data_in_use = InstanceBlocker::data_in_use(evidence.clone(), at);
        let unproven = InstanceBlocker::identity_unproven(evidence.clone(), at);

        assert_eq!(data_in_use.as_code(), "data_in_use");
        assert_eq!(unproven.as_code(), "identity_unproven");
        for blocker in [&data_in_use, &unproven] {
            assert_eq!(blocker.observed_at(), at);
            assert_eq!(blocker.evidence(), Some(&evidence));
        }
        assert_ne!(data_in_use, unproven);
    }
}
