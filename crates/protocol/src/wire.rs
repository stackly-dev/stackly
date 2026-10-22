//! Wire primitives every DTO shares: the `u64` newtypes, the millisecond
//! [`Timestamp`], and the [`ResourceRef`] that names one resource.
//!
//! typeshare rejects plain `u64` fields, so every 64 bit integer travels as
//! [`Revision`], [`Sequence`], or [`Count`]. `typeshare.toml` maps each to
//! `UInt64`, and decoding rejects values above [`MAX_SAFE_INTEGER`]. These
//! newtypes carry no `#[typeshare]` on purpose: the mapping is the contract.

use std::borrow::Cow;
use std::fmt;
use std::str::FromStr;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use typeshare::typeshare;

use crate::limits::MAX_SAFE_INTEGER;

/// Defines one transparent `u64` newtype capped at [`MAX_SAFE_INTEGER`].
macro_rules! safe_integer {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize)]
        #[serde(transparent)]
        pub struct $name(u64);

        impl $name {
            /// Zero.
            pub const ZERO: Self = Self(0);
            /// The largest value the wire carries, [`MAX_SAFE_INTEGER`].
            pub const MAX: Self = Self(MAX_SAFE_INTEGER);

            /// Wraps `value`, or returns `None` when it is above
            /// [`MAX_SAFE_INTEGER`].
            #[must_use]
            pub const fn new(value: u64) -> Option<Self> {
                if value <= MAX_SAFE_INTEGER { Some(Self(value)) } else { None }
            }

            /// Wraps `value`, capped at [`MAX_SAFE_INTEGER`].
            #[must_use]
            pub const fn saturating(value: u64) -> Self {
                if value <= MAX_SAFE_INTEGER { Self(value) } else { Self::MAX }
            }

            /// The plain number.
            #[must_use]
            pub const fn get(self) -> u64 {
                self.0
            }

            /// This value plus one, or `None` past [`MAX_SAFE_INTEGER`].
            #[must_use]
            pub const fn next(self) -> Option<Self> {
                Self::new(self.0 + 1)
            }
        }

        impl From<$name> for u64 {
            fn from(value: $name) -> Self {
                value.0
            }
        }

        impl TryFrom<u64> for $name {
            type Error = AboveSafeInteger;

            fn try_from(value: u64) -> Result<Self, Self::Error> {
                Self::new(value).ok_or(AboveSafeInteger(value))
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let value = u64::deserialize(deserializer)?;
                Self::new(value).ok_or_else(|| D::Error::custom(AboveSafeInteger(value)))
            }
        }

        impl JsonSchema for $name {
            fn schema_name() -> Cow<'static, str> {
                stringify!($name).into()
            }

            fn inline_schema() -> bool {
                true
            }

            fn json_schema(_: &mut SchemaGenerator) -> Schema {
                json_schema!({
                    "type": "integer",
                    "format": "uint64",
                    "minimum": 0,
                    "maximum": MAX_SAFE_INTEGER,
                })
            }
        }
    };
}

safe_integer! {
    /// Revision of one resource. It rises per resource and never falls; an
    /// event carries the revision after its change, and a delete event the
    /// last revision plus one.
    Revision
}

safe_integer! {
    /// Position of an event in one daemon run: 1 for the first event, rising
    /// by exactly one per published event. 0 means no event yet.
    Sequence
}

safe_integer! {
    /// A counter since daemon start, such as `connectionsTotal`.
    Count
}

/// A signed integer between minus and plus [`MAX_SAFE_INTEGER`], the signed
/// counterpart of [`Count`]: a setting value or a schema bound that may be
/// negative. `typeshare.toml` maps it to `Int64`, and decoding rejects a
/// value outside the range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize)]
#[serde(transparent)]
pub struct SignedSafeInteger(i64);

impl SignedSafeInteger {
    /// Zero.
    pub const ZERO: Self = Self(0);
    /// The smallest value the wire carries, minus [`MAX_SAFE_INTEGER`].
    pub const MIN: Self = Self(-(MAX_SAFE_INTEGER as i64));
    /// The largest value the wire carries, [`MAX_SAFE_INTEGER`].
    pub const MAX: Self = Self(MAX_SAFE_INTEGER as i64);

    /// Wraps `value`, or returns `None` when its size is above
    /// [`MAX_SAFE_INTEGER`].
    #[must_use]
    pub const fn new(value: i64) -> Option<Self> {
        if value.unsigned_abs() <= MAX_SAFE_INTEGER {
            Some(Self(value))
        } else {
            None
        }
    }

    /// Wraps `value`, clamped to the range of the wire.
    #[must_use]
    pub const fn saturating(value: i64) -> Self {
        if value < Self::MIN.0 {
            Self::MIN
        } else if value > Self::MAX.0 {
            Self::MAX
        } else {
            Self(value)
        }
    }

    /// The plain number.
    #[must_use]
    pub const fn get(self) -> i64 {
        self.0
    }
}

impl From<SignedSafeInteger> for i64 {
    fn from(value: SignedSafeInteger) -> Self {
        value.0
    }
}

impl TryFrom<i64> for SignedSafeInteger {
    type Error = OutsideSafeInteger;

    fn try_from(value: i64) -> Result<Self, Self::Error> {
        Self::new(value).ok_or(OutsideSafeInteger(value))
    }
}

impl fmt::Display for SignedSafeInteger {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl<'de> Deserialize<'de> for SignedSafeInteger {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = i64::deserialize(deserializer)?;
        Self::new(value).ok_or_else(|| D::Error::custom(OutsideSafeInteger(value)))
    }
}

impl JsonSchema for SignedSafeInteger {
    fn schema_name() -> Cow<'static, str> {
        "SignedSafeInteger".into()
    }

    fn inline_schema() -> bool {
        true
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "integer",
            "format": "int64",
            "minimum": -(MAX_SAFE_INTEGER as i64),
            "maximum": MAX_SAFE_INTEGER,
        })
    }
}

/// An `i64` whose size is above [`MAX_SAFE_INTEGER`] cannot travel on the
/// wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("integer {0} is outside -(2^53 - 1) to 2^53 - 1")]
pub struct OutsideSafeInteger(pub i64);

/// A `u64` above [`MAX_SAFE_INTEGER`] cannot travel on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("integer {0} is above 2^53 - 1")]
pub struct AboveSafeInteger(pub u64);

/// An instant on the wire: RFC 3339 in UTC with exactly three fractional
/// digits, such as `2026-10-18T03:04:05.123Z`.
///
/// Decoding accepts RFC 3339 with any offset and precision, then normalizes
/// to UTC and truncates to the millisecond, so a decoded value always encodes
/// to the canonical form. Swift reads it as `Date`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(jiff::Timestamp);

impl Timestamp {
    /// The Unix epoch, `1970-01-01T00:00:00.000Z`.
    pub const UNIX_EPOCH: Self = Self(jiff::Timestamp::UNIX_EPOCH);

    /// `instant` truncated toward zero to the millisecond. Every instant
    /// jiff holds works, `jiff::Timestamp::MIN` and `MAX` included.
    #[must_use]
    pub fn from_jiff(instant: jiff::Timestamp) -> Self {
        // Cutting only the nanoseconds keeps the seconds jiff already holds,
        // and jiff gives the nanoseconds the sign of the seconds, so the
        // result lies between the epoch and `instant`. Going through
        // `as_millisecond` instead fails near `MAX`, whose millisecond
        // count is outside what `from_millisecond` accepts.
        let nanosecond = instant.subsec_nanosecond() / 1_000_000 * 1_000_000;
        Self(
            jiff::Timestamp::new(instant.as_second(), nanosecond)
                .expect("an instant cut toward the epoch stays in jiff's range"),
        )
    }

    /// The instant as a `jiff::Timestamp`, always on a millisecond boundary.
    #[must_use]
    pub const fn as_jiff(self) -> jiff::Timestamp {
        self.0
    }

    /// Milliseconds since the Unix epoch.
    #[must_use]
    pub fn as_millisecond(self) -> i64 {
        self.0.as_millisecond()
    }
}

impl From<jiff::Timestamp> for Timestamp {
    fn from(instant: jiff::Timestamp) -> Self {
        Self::from_jiff(instant)
    }
}

impl fmt::Display for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:.3}", self.0)
    }
}

/// The text is not an RFC 3339 instant with an offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("timestamp is not an rfc 3339 instant with an offset")]
pub struct InvalidTimestamp;

impl FromStr for Timestamp {
    type Err = InvalidTimestamp;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        text.parse::<jiff::Timestamp>()
            .map(Self::from_jiff)
            .map_err(|_| InvalidTimestamp)
    }
}

impl Serialize for Timestamp {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Timestamp {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = Cow::<str>::deserialize(deserializer)?;
        text.parse().map_err(D::Error::custom)
    }
}

impl JsonSchema for Timestamp {
    fn schema_name() -> Cow<'static, str> {
        "Timestamp".into()
    }

    fn inline_schema() -> bool {
        true
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "string",
            "format": "date-time",
        })
    }
}

/// Names one resource, in errors and events.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResourceRef {
    /// What kind of resource this is.
    pub kind: ResourceKind,
    /// The canonical text of the resource key: a lowercase UUID for
    /// runtimes, instances, environments, operations, and backups,
    /// [`ResourceRef::MANAGER_ID`] for the manager. Spec 0009 fixes the
    /// form for providers.
    pub id: String,
}

impl ResourceRef {
    /// The `id` of the one [`ResourceKind::Manager`] resource (spec 0007).
    pub const MANAGER_ID: &str = "self";

    /// The manager itself, `{ kind: manager, id: "self" }`: the resource of
    /// an `update_manager` operation (spec 0007).
    #[must_use]
    pub fn manager() -> Self {
        Self {
            kind: ResourceKind::Manager,
            id: Self::MANAGER_ID.to_owned(),
        }
    }
}

/// The kinds of resource Stackly tracks. Open: a newer daemon may add
/// values, which older clients read as `unrecognized`.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ResourceKind {
    /// An installed runtime version.
    Runtime,
    /// A configured instance of a runtime.
    Instance,
    /// A group of instances.
    Environment,
    /// An asynchronous operation.
    Operation,
    /// A provider package.
    Provider,
    /// A backup of an instance.
    Backup,
    /// Stackly itself, the target of an `update_manager` operation; its
    /// `id` is always [`ResourceRef::MANAGER_ID`] (spec 0007).
    Manager,
    /// A secret reference (spec 0011); its `id` is the reference text,
    /// such as `local:erp/postgres/admin`.
    Secret,
    /// A kind this build does not know. The daemon never sends it.
    #[serde(other)]
    Unrecognized,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_integers_accept_max_safe_integer_and_reject_one_more() {
        let max: Sequence = serde_json::from_str("9007199254740991").unwrap();
        assert_eq!(max, Sequence::MAX);
        let err = serde_json::from_str::<Sequence>("9007199254740992").unwrap_err();
        assert!(err.to_string().contains("above 2^53 - 1"));
        assert_eq!(Revision::new(MAX_SAFE_INTEGER + 1), None);
        assert_eq!(Count::saturating(u64::MAX), Count::MAX);
        assert_eq!(Sequence::MAX.next(), None);
        assert_eq!(Sequence::ZERO.next(), Sequence::new(1));
    }

    #[test]
    fn signed_safe_integers_accept_the_range_and_reject_one_more_either_way() {
        let max: SignedSafeInteger = serde_json::from_str("9007199254740991").unwrap();
        assert_eq!(max, SignedSafeInteger::MAX);
        let min: SignedSafeInteger = serde_json::from_str("-9007199254740991").unwrap();
        assert_eq!(min, SignedSafeInteger::MIN);
        for text in ["9007199254740992", "-9007199254740992", "1.5"] {
            assert!(
                serde_json::from_str::<SignedSafeInteger>(text).is_err(),
                "{text}"
            );
        }
        assert_eq!(SignedSafeInteger::new(i64::MIN), None);
        assert_eq!(
            SignedSafeInteger::saturating(i64::MIN),
            SignedSafeInteger::MIN
        );
        assert_eq!(
            SignedSafeInteger::saturating(i64::MAX),
            SignedSafeInteger::MAX
        );
        assert_eq!(
            serde_json::to_string(&SignedSafeInteger::new(-7).unwrap()).unwrap(),
            "-7"
        );
    }

    #[test]
    fn safe_integers_encode_as_plain_json_numbers() {
        let revision = Revision::new(42).unwrap();
        assert_eq!(serde_json::to_string(&revision).unwrap(), "42");
        assert!(serde_json::from_str::<Count>("-1").is_err());
        assert!(serde_json::from_str::<Count>("1.5").is_err());
    }

    #[test]
    fn timestamps_encode_in_utc_with_exactly_three_fractional_digits() {
        let whole: Timestamp = "2026-10-18T03:04:05Z".parse().unwrap();
        assert_eq!(whole.to_string(), "2026-10-18T03:04:05.000Z");
        let fine: Timestamp = "2026-10-18T03:04:05.123456789Z".parse().unwrap();
        assert_eq!(fine.to_string(), "2026-10-18T03:04:05.123Z");
        assert_eq!(
            serde_json::to_string(&fine).unwrap(),
            "\"2026-10-18T03:04:05.123Z\""
        );
    }

    #[test]
    fn timestamps_with_an_offset_are_normalized_to_utc() {
        let offset: Timestamp = "2026-10-18T10:04:05.5+07:00".parse().unwrap();
        assert_eq!(offset.to_string(), "2026-10-18T03:04:05.500Z");
        let decoded: Timestamp = serde_json::from_str("\"2026-10-18T10:04:05+07:00\"").unwrap();
        assert_eq!(decoded.to_string(), "2026-10-18T03:04:05.000Z");
    }

    #[test]
    fn timestamps_without_an_offset_are_rejected() {
        assert_eq!(
            "2026-10-18T03:04:05".parse::<Timestamp>(),
            Err(InvalidTimestamp)
        );
        assert!(serde_json::from_str::<Timestamp>("1792292645").is_err());
    }

    #[test]
    fn a_decoded_timestamp_always_encodes_to_the_same_text() {
        let first: Timestamp = "2026-10-18T03:04:05.999999+00:00".parse().unwrap();
        let text = first.to_string();
        let second: Timestamp = text.parse().unwrap();
        assert_eq!(first, second);
        assert_eq!(second.to_string(), text);
    }

    #[test]
    fn instants_up_to_jiff_max_truncate_without_panicking() {
        let max = Timestamp::from_jiff(jiff::Timestamp::MAX);
        assert_eq!(max.to_string(), "9999-12-30T22:00:00.999Z");
        let edge: Timestamp = "9999-12-30T22:00:00.001Z".parse().unwrap();
        assert_eq!(edge.to_string(), "9999-12-30T22:00:00.001Z");
        let fine: Timestamp = "9999-12-30T22:00:00.999999999Z".parse().unwrap();
        assert_eq!(fine, max);
        assert_eq!(fine.to_string().parse::<Timestamp>(), Ok(max));
        assert_eq!(max.as_millisecond(), 253_402_207_200_999);
    }

    #[test]
    fn instants_down_to_jiff_min_truncate_toward_the_epoch() {
        let min = Timestamp::from_jiff(jiff::Timestamp::MIN);
        assert_eq!(min.as_jiff(), jiff::Timestamp::MIN);
        assert_eq!(min.to_string().parse::<Timestamp>(), Ok(min));
        let before_epoch = jiff::Timestamp::new(-1, -999_999_999).unwrap();
        let cut = Timestamp::from_jiff(before_epoch);
        assert_eq!(cut.as_millisecond(), -1_999);
        assert_eq!(cut.to_string(), "1969-12-31T23:59:58.001Z");
        let just_before = jiff::Timestamp::new(0, -1).unwrap();
        assert_eq!(Timestamp::from_jiff(just_before), Timestamp::UNIX_EPOCH);
    }

    #[test]
    fn every_nanosecond_step_near_the_edges_truncates_to_a_millisecond() {
        for second in [
            jiff::Timestamp::MAX.as_second(),
            jiff::Timestamp::MIN.as_second() + 1,
            -1,
            0,
        ] {
            for nanosecond in [-999_999_999, -1_000_001, -1, 0, 1, 999_999, 999_999_999] {
                let Ok(instant) = jiff::Timestamp::new(second, nanosecond) else {
                    continue;
                };
                let cut = Timestamp::from_jiff(instant).as_jiff();
                assert_eq!(cut.subsec_nanosecond() % 1_000_000, 0, "{instant}");
                assert!(
                    (cut.as_nanosecond() - instant.as_nanosecond()).abs() < 1_000_000,
                    "{instant}"
                );
            }
        }
    }

    #[test]
    fn foreign_resource_kinds_decode_as_unrecognized() {
        let kind: ResourceKind = serde_json::from_str("\"volume\"").unwrap();
        assert_eq!(kind, ResourceKind::Unrecognized);
        let known: ResourceKind = serde_json::from_str("\"instance\"").unwrap();
        assert_eq!(known, ResourceKind::Instance);
    }

    #[test]
    fn the_manager_is_one_resource_with_the_id_self() {
        let json = serde_json::to_value(ResourceRef::manager()).unwrap();
        assert_eq!(json, serde_json::json!({ "kind": "manager", "id": "self" }));
    }
}
