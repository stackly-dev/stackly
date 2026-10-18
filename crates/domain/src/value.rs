//! Validated value types: names, versions, and digests.

use std::fmt;
use std::str::FromStr;

use crate::error::{ValueError, ValueKind};
use crate::text::serde_as_text;

/// Longest [`InstanceName`] or [`SoftwareName`], and each part of a
/// [`ProviderName`], in characters.
const NAME_MAX: usize = 63;
/// Longest [`DisplayName`], in characters.
const DISPLAY_NAME_MAX: usize = 128;
/// Longest [`RevisionLabel`], in characters.
const REVISION_LABEL_MAX: usize = 64;
/// Longest [`ExactVersion`], in characters.
const EXACT_VERSION_MAX: usize = 64;
/// Number of hex characters in a [`Sha256Digest`].
const SHA256_HEX_LEN: usize = 64;

const NAME_RULE: &str = "must be 1 to 63 characters: a lowercase letter, then lowercase letters, \
                         digits, and single hyphens, not ending in a hyphen";

/// Whether `s` is 1 to [`NAME_MAX`] characters matching
/// `^[a-z]([a-z0-9]*(-[a-z0-9]+)*)?$` (VAL-001).
fn is_name(s: &str) -> bool {
    let bytes = s.as_bytes();
    bytes.first().is_some_and(u8::is_ascii_lowercase)
        && bytes.len() <= NAME_MAX
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
        && !s.ends_with('-')
        && !s.contains("--")
}

/// Whether `s` is one or more ASCII digits.
fn is_digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

fn check_name(s: &str) -> Result<(), &'static str> {
    if is_name(s) { Ok(()) } else { Err(NAME_RULE) }
}

fn check_display_name(s: &str) -> Result<(), &'static str> {
    let len = s.chars().count();
    if (1..=DISPLAY_NAME_MAX).contains(&len) && !s.chars().any(char::is_control) {
        Ok(())
    } else {
        Err("must be 1 to 128 characters with no control characters")
    }
}

fn check_revision_label(s: &str) -> Result<(), &'static str> {
    let bytes = s.as_bytes();
    let allowed = |b: &u8| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'+' | b'_' | b'-');
    if bytes.len() <= REVISION_LABEL_MAX
        && bytes.first().is_some_and(u8::is_ascii_alphanumeric)
        && bytes.iter().all(allowed)
        && !s.contains("..")
    {
        Ok(())
    } else {
        Err(
            "must be 1 to 64 letters, digits, and . + _ -, starting with a letter or digit, \
             without ..",
        )
    }
}

/// Checks `^[0-9]+(\.[0-9]+)+([+-][0-9A-Za-z.]+)?$`, at most
/// [`EXACT_VERSION_MAX`] characters (the pattern of spec 0003).
fn check_exact_version(s: &str) -> Result<(), &'static str> {
    let (release, suffix) = match s.split_once(['+', '-']) {
        Some((release, suffix)) => (release, Some(suffix)),
        None => (s, None),
    };
    let suffix_ok = suffix.is_none_or(|suffix| {
        !suffix.is_empty()
            && suffix
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'.')
    });
    if s.len() <= EXACT_VERSION_MAX
        && release.split('.').count() >= 2
        && release.split('.').all(is_digits)
        && suffix_ok
    {
        Ok(())
    } else {
        Err(
            "must be at most 64 characters of dotted numbers such as 17.6, with an optional \
             + or - suffix of letters, digits, and dots",
        )
    }
}

fn check_runtime_line(s: &str) -> Result<(), &'static str> {
    if (1..=2).contains(&s.split('.').count()) && s.split('.').all(is_digits) {
        Ok(())
    } else {
        Err("must be a major version or major.minor, such as 17 or 8.4")
    }
}

/// Defines a newtype over a validated `String` whose text form is the
/// string itself. Callers add `#[derive(PartialOrd, Ord)]` when ordering
/// makes sense for the value.
macro_rules! text_value {
    ($(#[$meta:meta])* $name:ident, $kind:ident, $expecting:literal, $check:path) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash)]
        pub struct $name(String);

        impl $name {
            /// Returns the text form.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl FromStr for $name {
            type Err = ValueError;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                $check(s).map_err(|reason| ValueError::new(ValueKind::$kind, reason))?;
                Ok(Self(s.to_owned()))
            }
        }

        serde_as_text!($name => $expecting);
    };
}

text_value! {
    /// The name of an instance: 1 to 63 characters, a lowercase letter,
    /// then lowercase letters, digits, and single hyphens, not ending in a
    /// hyphen (VAL-001).
    ///
    /// ```
    /// use stackly_domain::InstanceName;
    ///
    /// assert!("orders-db".parse::<InstanceName>().is_ok());
    /// assert!("Orders".parse::<InstanceName>().is_err());
    /// ```
    #[derive(PartialOrd, Ord)]
    InstanceName, InstanceName, "an instance name", check_name
}

text_value! {
    /// The optional human name of an instance: 1 to 128 Unicode characters
    /// with no control characters.
    #[derive(PartialOrd, Ord)]
    DisplayName, DisplayName, "a display name", check_display_name
}

text_value! {
    /// The name of a software, such as `postgresql`, with the same rules
    /// as [`InstanceName`].
    #[derive(PartialOrd, Ord)]
    SoftwareName, SoftwareName, "a software name", check_name
}

text_value! {
    /// The revision of a provider: 1 to 64 letters, digits, and `. + _ -`,
    /// starting with a letter or digit, without `..` (VAL-002).
    ///
    /// The rules keep it a single safe path component.
    #[derive(PartialOrd, Ord)]
    RevisionLabel, RevisionLabel, "a revision label", check_revision_label
}

text_value! {
    /// An exact runtime version, such as `17.6` or `8.4.3`, with an
    /// optional `+` or `-` suffix (the pattern of spec 0003).
    ///
    /// Versions compare only for equality. They have no order, because
    /// providers order versions in their own ways.
    ExactVersion, ExactVersion, "an exact version", check_exact_version
}

text_value! {
    /// A runtime line: a major version or `major.minor`, such as `17`,
    /// `8.4`, or `26.3`.
    #[derive(PartialOrd, Ord)]
    RuntimeLine, RuntimeLine, "a runtime line", check_runtime_line
}

impl RuntimeLine {
    /// Returns whether `version` belongs to this line, that is, whether it
    /// starts with `<line>.`.
    ///
    /// ```
    /// use stackly_domain::{ExactVersion, RuntimeLine};
    ///
    /// let line: RuntimeLine = "8.4".parse()?;
    /// assert!(line.contains(&"8.4.3".parse::<ExactVersion>()?));
    /// assert!(!line.contains(&"8.40.1".parse::<ExactVersion>()?));
    /// # Ok::<(), stackly_domain::ValueError>(())
    /// ```
    #[must_use]
    pub fn contains(&self, version: &ExactVersion) -> bool {
        version
            .as_str()
            .strip_prefix(self.as_str())
            .is_some_and(|rest| rest.starts_with('.'))
    }
}

/// The name of a provider, `<namespace>/<name>`, such as
/// `official/postgresql`. Each part follows the rules of [`InstanceName`]
/// (VAL-002).
///
/// ```
/// use stackly_domain::ProviderName;
///
/// let provider: ProviderName = "official/postgresql".parse()?;
/// assert_eq!(provider.namespace(), "official");
/// assert_eq!(provider.name(), "postgresql");
/// # Ok::<(), stackly_domain::ValueError>(())
/// ```
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ProviderName {
    text: String,
    /// Byte index of the `/`, always an ASCII character inside `text`.
    slash: usize,
}

impl ProviderName {
    /// Returns the namespace, the part before the `/`.
    #[must_use]
    pub fn namespace(&self) -> &str {
        &self.text[..self.slash]
    }

    /// Returns the name, the part after the `/`.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.text[self.slash + 1..]
    }

    /// Returns the text form, `<namespace>/<name>`.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.text
    }
}

impl fmt::Debug for ProviderName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("ProviderName").field(&self.text).finish()
    }
}

impl fmt::Display for ProviderName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

impl FromStr for ProviderName {
    type Err = ValueError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.split_once('/') {
            Some((namespace, name)) if is_name(namespace) && is_name(name) => Ok(Self {
                text: s.to_owned(),
                slash: namespace.len(),
            }),
            _ => Err(ValueError::new(
                ValueKind::ProviderName,
                "must be <namespace>/<name>, each part 1 to 63 characters: a lowercase letter, \
                 then lowercase letters, digits, and single hyphens",
            )),
        }
    }
}

/// A SHA-256 digest, written as 64 lowercase hex characters. Parsing
/// accepts either letter case.
///
/// ```
/// use stackly_domain::Sha256Digest;
///
/// let text = "AB".repeat(32);
/// let digest: Sha256Digest = text.parse()?;
/// assert_eq!(digest.to_string(), "ab".repeat(32));
/// # Ok::<(), stackly_domain::ValueError>(())
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Sha256Digest([u8; 32]);

impl Sha256Digest {
    /// Wraps the 32 bytes of a digest.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Returns the 32 bytes of the digest.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Debug for Sha256Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Sha256Digest")
            .field(&format_args!("{self}"))
            .finish()
    }
}

impl fmt::Display for Sha256Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.iter().try_for_each(|byte| write!(f, "{byte:02x}"))
    }
}

impl FromStr for Sha256Digest {
    type Err = ValueError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let invalid = || ValueError::new(ValueKind::Sha256Digest, "must be 64 hex characters");
        if s.len() != SHA256_HEX_LEN {
            return Err(invalid());
        }
        let (pairs, _) = s.as_bytes().as_chunks::<2>();
        let mut bytes = [0_u8; 32];
        for (byte, &[high, low]) in bytes.iter_mut().zip(pairs) {
            let high = hex_value(high).ok_or_else(invalid)?;
            let low = hex_value(low).ok_or_else(invalid)?;
            *byte = (high << 4) | low;
        }
        Ok(Self(bytes))
    }
}

/// Returns the value of one hex digit of either letter case.
const fn hex_value(digit: u8) -> Option<u8> {
    match digit {
        b'0'..=b'9' => Some(digit - b'0'),
        b'a'..=b'f' => Some(digit - b'a' + 10),
        b'A'..=b'F' => Some(digit - b'A' + 10),
        _ => None,
    }
}

serde_as_text! {
    ProviderName => "a provider name such as official/postgresql",
    Sha256Digest => "a sha256 digest of 64 hex characters",
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_rejects<T>(kind: ValueKind, cases: &[&str])
    where
        T: FromStr<Err = ValueError> + fmt::Debug,
    {
        for text in cases {
            let err = text.parse::<T>().unwrap_err();
            assert_eq!(err.kind(), kind, "{text:?}");
        }
    }

    fn assert_accepts<T>(cases: &[&str])
    where
        T: FromStr<Err = ValueError> + fmt::Display,
    {
        for text in cases {
            let value: T = text.parse().unwrap_or_else(|err| panic!("{text:?}: {err}"));
            assert_eq!(value.to_string(), *text);
        }
    }

    #[test]
    fn instance_names_follow_the_lowercase_hyphen_pattern() {
        let longest = format!("a{}", "b".repeat(62));
        assert_accepts::<InstanceName>(&["a", "orders-db", "pg17", "a-1-b", &longest]);

        let too_long = format!("a{}", "b".repeat(63));
        assert_rejects::<InstanceName>(
            ValueKind::InstanceName,
            &[
                "",
                "1db",
                "-db",
                "db-",
                "or--ders",
                "Orders",
                "orders_db",
                "orders.db",
                "ördërs",
                "a b",
                &too_long,
            ],
        );
    }

    #[test]
    fn software_names_use_the_instance_name_rules() {
        assert_accepts::<SoftwareName>(&["postgresql", "valkey", "click-house"]);
        assert_rejects::<SoftwareName>(
            ValueKind::SoftwareName,
            &["", "PostgreSQL", "postgresql-", "9db"],
        );
    }

    #[test]
    fn display_names_allow_unicode_up_to_128_characters_without_controls() {
        let longest = "é".repeat(128);
        assert_accepts::<DisplayName>(&["Orders database", "データベース", &longest]);

        let too_long = "é".repeat(129);
        assert_rejects::<DisplayName>(
            ValueKind::DisplayName,
            &["", "tab\there", "new\nline", "nul\0", &too_long],
        );
    }

    #[test]
    fn provider_names_are_two_name_parts_around_one_slash() {
        let provider: ProviderName = "official/postgresql".parse().unwrap();

        assert_eq!(provider.namespace(), "official");
        assert_eq!(provider.name(), "postgresql");
        assert_eq!(provider.as_str(), "official/postgresql");
        assert_accepts::<ProviderName>(&["a/b", "my-org/click-house"]);
        assert_rejects::<ProviderName>(
            ValueKind::ProviderName,
            &[
                "",
                "postgresql",
                "/postgresql",
                "official/",
                "official/postgresql/17",
                "Official/postgresql",
                "official/../etc",
            ],
        );
    }

    #[test]
    fn revision_labels_are_one_safe_path_component() {
        let longest = "1".repeat(64);
        assert_accepts::<RevisionLabel>(&["1", "2026.10.1", "v1+build_7", "rc-1", &longest]);

        let too_long = "1".repeat(65);
        assert_rejects::<RevisionLabel>(
            ValueKind::RevisionLabel,
            &[
                "", ".", "..", ".hidden", "-rc", "1..2", "a/b", "a b", "ä", &too_long,
            ],
        );
    }

    #[test]
    fn exact_versions_follow_the_spec_0003_pattern() {
        assert_accepts::<ExactVersion>(&[
            "17.6",
            "8.4.3",
            "26.3.1.12",
            "1.0-rc.1",
            "1.0+build.5",
            "0.1.0",
        ]);

        let too_long = format!("1.{}", "0".repeat(63));
        assert_rejects::<ExactVersion>(
            ValueKind::ExactVersion,
            &[
                "",
                "17",
                "17.",
                ".6",
                "17..6",
                "v17.6",
                "17.6-",
                "17.6+",
                "17.6-rc-1",
                "17.6+a+b",
                "17.6_1",
                "17.x",
                &too_long,
            ],
        );
    }

    #[test]
    fn runtime_lines_are_major_or_major_minor() {
        assert_accepts::<RuntimeLine>(&["17", "8.4", "26.3"]);
        assert_rejects::<RuntimeLine>(
            ValueKind::RuntimeLine,
            &["", "17.", ".4", "8.4.3", "8.x", "v17", "17-rc"],
        );
    }

    #[test]
    fn a_line_contains_only_versions_that_start_with_the_line_and_a_dot() {
        let line: RuntimeLine = "17".parse().unwrap();
        let minor: RuntimeLine = "8.4".parse().unwrap();
        let version = |text: &str| text.parse::<ExactVersion>().unwrap();

        assert!(line.contains(&version("17.6")));
        assert!(line.contains(&version("17.0-rc.1")));
        assert!(!line.contains(&version("170.1")));
        assert!(!line.contains(&version("1.17")));
        assert!(minor.contains(&version("8.4.3")));
        assert!(!minor.contains(&version("8.40.1")));
        assert!(!minor.contains(&version("8.4-rc.1")));
    }

    #[test]
    fn sha256_digests_parse_either_case_and_print_lowercase() {
        let upper = "AB01".repeat(16);
        let digest: Sha256Digest = upper.parse().unwrap();

        assert_eq!(digest.to_string(), upper.to_lowercase());
        assert_eq!(digest.as_bytes()[..2], [0xab, 0x01]);
        assert_eq!(Sha256Digest::from_bytes(*digest.as_bytes()), digest);
        assert_eq!(
            format!("{digest:?}"),
            format!("Sha256Digest({})", upper.to_lowercase())
        );

        let short = "a".repeat(63);
        let long = "a".repeat(65);
        let not_hex = format!("{}g", "a".repeat(63));
        let multibyte = format!("{}é", "a".repeat(62));
        assert_rejects::<Sha256Digest>(
            ValueKind::Sha256Digest,
            &["", &short, &long, &not_hex, &multibyte],
        );
    }

    #[test]
    fn value_types_serialize_as_text_and_validate_on_deserialize() {
        let name: InstanceName = serde_json::from_str("\"orders-db\"").unwrap();
        let provider: ProviderName = serde_json::from_str("\"official/postgresql\"").unwrap();

        assert_eq!(serde_json::to_string(&name).unwrap(), "\"orders-db\"");
        assert_eq!(
            serde_json::to_string(&provider).unwrap(),
            "\"official/postgresql\""
        );
        let err = serde_json::from_str::<InstanceName>("\"Orders\"").unwrap_err();
        assert!(
            err.to_string().starts_with("invalid instance name"),
            "{err}"
        );
        let err = serde_json::from_str::<ExactVersion>("17").unwrap_err();
        assert!(err.to_string().contains("an exact version"), "{err}");
    }
}
