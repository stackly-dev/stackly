//! The selector a user names a runtime with (spec 0018, "Resolusi
//! selector").

use std::fmt;
use std::str::FromStr;

use crate::error::{ValueError, ValueKind};
use crate::text::serde_as_text;
use crate::value::{ExactVersion, RuntimeLine, Sha256Digest};

/// Most bytes in the text of a selector.
pub const SELECTOR_MAX: usize = 80;

const SHA256_PREFIX: &str = "sha256:";

/// What a user typed after `@` to pick a runtime: `stable`, a digest, or
/// text that names a version, a line, or both.
///
/// The text is 1 to 80 bytes of `[A-Za-z0-9.+:_-]`. `stable` and
/// `sha256:<64 lowercase hex>` are the two reserved shapes; any other text
/// is kept as it is, with the readings the domain can give it: an
/// [`ExactVersion`] when it passes that check and a [`RuntimeLine`] when it
/// passes that one. Text that passes neither is not a selector. Matching
/// them against a catalog never orders versions, and a selector is never
/// stored on a runtime.
///
/// ```
/// use stackly_domain::RuntimeSelector;
///
/// assert_eq!("stable".parse::<RuntimeSelector>()?, RuntimeSelector::Stable);
/// let both: RuntimeSelector = "17".parse()?;
/// assert!(matches!(both, RuntimeSelector::Text { version: None, line: Some(_), .. }));
/// assert!("17 6".parse::<RuntimeSelector>().is_err());
/// # Ok::<(), stackly_domain::ValueError>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RuntimeSelector {
    /// `stable`: the artifact the catalog marks stable for the host.
    Stable,
    /// `sha256:<hex>`: the artifact with exactly this digest.
    Sha256(Sha256Digest),
    /// Any other text, matched against exact versions first, then lines.
    #[non_exhaustive]
    Text {
        /// The text as typed.
        text: String,
        /// The reading as an exact version, if it is one.
        version: Option<ExactVersion>,
        /// The reading as a runtime line, if it is one.
        line: Option<RuntimeLine>,
    },
}

impl fmt::Display for RuntimeSelector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Stable => f.write_str("stable"),
            Self::Sha256(digest) => write!(f, "{SHA256_PREFIX}{digest}"),
            Self::Text { text, .. } => f.write_str(text),
        }
    }
}

impl FromStr for RuntimeSelector {
    type Err = ValueError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let invalid = |reason| ValueError::new(ValueKind::RuntimeSelector, reason);
        if s.is_empty() || s.len() > SELECTOR_MAX {
            return Err(invalid("must be 1 to 80 bytes"));
        }
        if !s
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b".+:_-".contains(&byte))
        {
            return Err(invalid("may only hold letters, digits, and . + : _ -"));
        }
        if s == "stable" {
            return Ok(Self::Stable);
        }
        if let Some(hex) = s.strip_prefix(SHA256_PREFIX) {
            return if hex.len() == 64
                && hex
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            {
                hex.parse()
                    .map(Self::Sha256)
                    .map_err(|_| invalid("the digest must be 64 lowercase hex characters"))
            } else {
                Err(invalid("the digest must be 64 lowercase hex characters"))
            };
        }
        let version = s.parse::<ExactVersion>().ok();
        let line = s.parse::<RuntimeLine>().ok();
        if version.is_none() && line.is_none() {
            return Err(invalid(
                "must be stable, sha256:<digest>, a version such as 17.6, or a line such as 17",
            ));
        }
        Ok(Self::Text {
            text: s.to_owned(),
            version,
            line,
        })
    }
}

serde_as_text!(RuntimeSelector => "a runtime selector");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_and_digest_are_the_reserved_shapes() {
        assert_eq!(
            "stable".parse::<RuntimeSelector>().unwrap(),
            RuntimeSelector::Stable
        );
        let hex = "ab".repeat(32);
        let parsed: RuntimeSelector = format!("sha256:{hex}").parse().unwrap();
        assert!(matches!(&parsed, RuntimeSelector::Sha256(d) if d.to_string() == hex));
        assert_eq!(parsed.to_string(), format!("sha256:{hex}"));
    }

    #[test]
    fn a_digest_selector_needs_64_lowercase_hex_characters() {
        for text in [
            "sha256:".to_owned(),
            format!("sha256:{}", "a".repeat(63)),
            format!("sha256:{}", "A".repeat(64)),
            format!("sha256:{}", "g".repeat(64)),
        ] {
            assert!(text.parse::<RuntimeSelector>().is_err(), "{text}");
        }
    }

    #[test]
    fn text_keeps_its_version_and_line_readings() {
        let line: RuntimeSelector = "17".parse().unwrap();
        assert!(matches!(
            line,
            RuntimeSelector::Text {
                version: None,
                line: Some(_),
                ..
            }
        ));
        let version: RuntimeSelector = "17.6".parse().unwrap();
        assert!(matches!(
            version,
            RuntimeSelector::Text {
                version: Some(_),
                line: Some(_),
                ..
            }
        ));
        let exact: RuntimeSelector = "2.15.1".parse().unwrap();
        assert!(matches!(
            exact,
            RuntimeSelector::Text {
                version: Some(_),
                line: None,
                ..
            }
        ));
    }

    #[test]
    fn text_outside_the_charset_or_the_bounds_is_rejected() {
        let long = "1".repeat(81);
        for text in ["", "latest", "17 6", "17/6", "é", "1.2\n", long.as_str()] {
            assert!(text.parse::<RuntimeSelector>().is_err(), "{text:?}");
        }
        assert!("1".repeat(80).parse::<RuntimeSelector>().is_ok());
    }

    #[test]
    fn a_selector_travels_as_text_through_serde() {
        let selector: RuntimeSelector = serde_json::from_str("\"8.4\"").unwrap();
        assert_eq!(serde_json::to_string(&selector).unwrap(), "\"8.4\"");
        assert!(serde_json::from_str::<RuntimeSelector>("\"nope\"").is_err());
    }
}
