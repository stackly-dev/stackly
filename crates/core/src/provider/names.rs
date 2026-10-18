//! Name and charset rules of the provider manifest (spec 0009, "Nama dan
//! charset").

use std::fmt;
use std::str::FromStr;

use serde::Serialize;

/// Longest token segment, in bytes.
const SEGMENT_MAX: usize = 64;
/// Longest manifest name (endpoint, setting, config file, alias, loop
/// variable), in bytes.
const NAME_MAX: usize = 32;
/// Longest literal path component, in bytes.
const COMPONENT_MAX: usize = 64;
/// Longest env key, in bytes.
const ENV_KEY_MAX: usize = 64;

/// Names a loop variable may not take, because they are namespaces.
pub(crate) const RESERVED_LOOP_NAMES: [&str; 7] = [
    "instance", "runtime", "endpoint", "settings", "secret", "provider", "loop",
];

/// Whether `s` is a token segment: `^[a-zA-Z][a-zA-Z0-9]{0,63}$`.
pub(crate) fn is_segment(s: &str) -> bool {
    let bytes = s.as_bytes();
    bytes.first().is_some_and(u8::is_ascii_alphabetic)
        && bytes.len() <= SEGMENT_MAX
        && bytes.iter().all(u8::is_ascii_alphanumeric)
}

/// Whether `s` is a manifest name: `^[a-z][a-zA-Z0-9]{0,31}$`.
pub(crate) fn is_name(s: &str) -> bool {
    let bytes = s.as_bytes();
    bytes.first().is_some_and(u8::is_ascii_lowercase)
        && bytes.len() <= NAME_MAX
        && bytes.iter().all(u8::is_ascii_alphanumeric)
}

/// The rule of [`is_name`], for messages.
pub(crate) const NAME_RULE: &str =
    "must be 1 to 32 characters: a lowercase letter, then letters and digits";

/// Whether `s` is a literal path component: `^[A-Za-z0-9._-]{1,64}$`,
/// other than `.` and `..`.
pub(crate) fn is_component(s: &str) -> bool {
    (1..=COMPONENT_MAX).contains(&s.len())
        && s.bytes().all(is_component_byte)
        && s != "."
        && s != ".."
}

/// The rule of [`is_component`], for messages.
pub(crate) const COMPONENT_RULE: &str =
    "path components must be 1 to 64 letters, digits, and . _ -, other than . and ..";

/// Whether `b` may appear in a literal path component or socket name.
pub(crate) const fn is_component_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-')
}

/// Whether `s` is a template file name: `^[a-z0-9][a-z0-9._-]{0,63}\.tmpl$`.
pub(crate) fn is_template_name(s: &str) -> bool {
    let Some(stem) = s.strip_suffix(".tmpl") else {
        return false;
    };
    let lower = |b: &u8| b.is_ascii_lowercase() || b.is_ascii_digit();
    let bytes = stem.as_bytes();
    bytes.first().is_some_and(lower)
        && bytes.len() <= 64
        && bytes
            .iter()
            .all(|b| lower(b) || matches!(b, b'.' | b'_' | b'-'))
}

/// Why an env key is rejected, or `None` when it is allowed: it matches
/// `^[A-Z][A-Z0-9_]{0,63}$`, is not `PATH`, `HOME`, or `TMPDIR`, and does
/// not start with `DYLD_` or `LD_`.
pub(crate) fn env_key_problem(key: &str) -> Option<&'static str> {
    let bytes = key.as_bytes();
    let shape = bytes.first().is_some_and(u8::is_ascii_uppercase)
        && bytes.len() <= ENV_KEY_MAX
        && bytes
            .iter()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || *b == b'_');
    if !shape {
        Some(
            "env keys must be 1 to 64 characters: an uppercase letter, then uppercase letters, digits, and _",
        )
    } else if matches!(key, "PATH" | "HOME" | "TMPDIR") {
        Some("env keys PATH, HOME, and TMPDIR come from the plan and cannot be overridden")
    } else if key.starts_with("DYLD_") || key.starts_with("LD_") {
        Some("env keys starting with DYLD_ or LD_ are not allowed")
    } else {
        None
    }
}

/// Whether `c` is a control character the settings dialect rejects in
/// text: U+0000 to U+001F and U+007F to U+009F.
pub(crate) fn is_control(c: char) -> bool {
    c.is_control()
}

/// The name of a settings field or item field, such as `readyDelayMs`:
/// `^[a-z][a-zA-Z0-9]{0,31}$`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct SettingName(String);

impl SettingName {
    /// Returns the name as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SettingName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Error returned when text is not a valid [`SettingName`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid setting name: {NAME_RULE}")]
#[non_exhaustive]
pub struct InvalidSettingName;

impl FromStr for SettingName {
    type Err = InvalidSettingName;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if is_name(s) {
            Ok(Self(s.to_owned()))
        } else {
            Err(InvalidSettingName)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_names_start_lowercase_and_hold_only_letters_and_digits() {
        assert!(is_name("client"));
        assert!(is_name("readyDelayMs"));
        assert!(is_name(&"a".repeat(32)));
        assert!(!is_name(&"a".repeat(33)));
        assert!(!is_name("client-tls"));
        assert!(!is_name("Client"));
        assert!(!is_name("9lives"));
        assert!(!is_name(""));
    }

    #[test]
    fn token_segments_may_start_uppercase_but_never_with_a_digit() {
        assert!(is_segment("shortKey"));
        assert!(is_segment("X"));
        assert!(!is_segment("1x"));
        assert!(!is_segment("a_b"));
        assert!(!is_segment(&"a".repeat(65)));
    }

    #[test]
    fn path_components_reject_dots_and_separators() {
        assert!(is_component("nats.conf"));
        assert!(is_component("FAKE_VERSION"));
        assert!(!is_component("."));
        assert!(!is_component(".."));
        assert!(!is_component("a/b"));
        assert!(!is_component("a b"));
        assert!(!is_component(""));
        assert!(!is_component(&"a".repeat(65)));
    }

    #[test]
    fn template_names_end_in_tmpl_and_start_with_a_lowercase_letter_or_digit() {
        assert!(is_template_name("nats.conf.tmpl"));
        assert!(is_template_name("0.tmpl"));
        assert!(!is_template_name(".hidden.tmpl"));
        assert!(!is_template_name("Upper.tmpl"));
        assert!(!is_template_name("nats.conf"));
        assert!(!is_template_name(".tmpl"));
    }

    #[test]
    fn env_keys_reject_the_base_env_and_loader_variables() {
        assert_eq!(env_key_problem("FAKE_MODE"), None);
        assert!(env_key_problem("PATH").is_some());
        assert!(env_key_problem("HOME").is_some());
        assert!(env_key_problem("TMPDIR").is_some());
        assert!(env_key_problem("DYLD_INSERT_LIBRARIES").is_some());
        assert!(env_key_problem("LD_PRELOAD").is_some());
        assert!(env_key_problem("lower").is_some());
        assert!(env_key_problem("_X").is_some());
    }

    #[test]
    fn setting_names_round_trip_through_text() {
        let name: SettingName = "authToken".parse().unwrap();
        assert_eq!(name.as_str(), "authToken");
        assert_eq!(name.to_string(), "authToken");
        assert!("auth-token".parse::<SettingName>().is_err());
    }
}
