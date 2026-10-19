//! How a secret value reaches a provider process (spec 0011, OQ-008).
//!
//! A provider package picks the highest mechanism its tool supports, by
//! where it puts a secret token in its manifest (spec 0009):
//!
//! 1. `stdin`: [`stdin_payload`], written once to the child's stdin, then
//!    the pipe is closed.
//! 2. `hash`: the config holds [`hash`] of the value, never the value.
//! 3. `file_once`: a [`SecretFile`] in the instance's `secrets/` folder,
//!    removed when its guard drops, after the step that reads it.
//! 4. `env`: [`env_pair`] in the child's explicit environment, never
//!    inherited from the daemon's.
//! 5. `config_plaintext`: a `sensitive` config rendered with
//!    [`render_secret_placeholder`] into the `secrets/` folder before every
//!    start and reload, removed after a clean stop with
//!    [`remove_secret_material`], and swept for every instance that is not
//!    running with [`sweep_secret_material`].
//!
//! Never argv: [`ensure_not_in_argv`] is the last guard before a spawn.
//! Every file this module deletes goes through
//! [`remove_owned_tree`](crate::remove_owned_tree), which never follows a
//! symlink and never leaves its owned root.

mod argv;
mod material;

use std::ffi::OsStr;
use std::fmt::Write as _;

use sha2::{Digest, Sha256};
use stackly_platform::SecretValue;

use crate::provider::TemplateFormat;

pub use argv::{SecretInArgv, SecretInArgvReason, ensure_not_in_argv};
pub use material::{
    DeliveryError, SecretFile, SweepReport, counts_as_running, remove_secret_material,
    sweep_secret_material,
};

/// Returns the bytes a child reads from stdin (mechanism 1): exactly the
/// value, with no newline. The caller writes them once and closes the pipe.
#[must_use]
pub fn stdin_payload(value: &SecretValue) -> &[u8] {
    value.expose().as_bytes()
}

/// A hash a config may hold in place of a secret (mechanism 2). A hash is
/// not a secret, but a weak typed value can be guessed from it offline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum HashAlgo {
    /// SHA-256 as 64 lowercase hex characters, as ClickHouse takes it.
    Sha256Hex,
}

impl HashAlgo {
    /// Returns the stable snake_case code, for example `sha256_hex`.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::Sha256Hex => "sha256_hex",
        }
    }
}

/// Returns the hash of a value with `algo` (mechanism 2).
#[must_use]
pub fn hash(algo: HashAlgo, value: &SecretValue) -> String {
    match algo {
        HashAlgo::Sha256Hex => {
            let digest = Sha256::digest(value.expose().as_bytes());
            let mut hex = String::with_capacity(digest.len() * 2);
            for byte in digest {
                let _ = write!(hex, "{byte:02x}");
            }
            hex
        }
    }
}

/// Returns one entry of a child's explicit environment (mechanism 4): the
/// variable name and the value. The caller adds it to the environment it
/// builds for the child, which never inherits the daemon's.
#[must_use]
pub fn env_pair<'a>(name: &'a str, value: &'a SecretValue) -> (&'a OsStr, &'a OsStr) {
    (OsStr::new(name), OsStr::new(value.expose()))
}

/// Returns the text a secret segment of a config template becomes: the
/// value escaped for `format`, as the renderer escapes any other text
/// (mechanism 5, and every `sensitive` template of spec 0009). Returns
/// `None` when the format cannot hold the value, for example a NATS config
/// and a value with `"` or `$`.
#[must_use]
pub fn render_secret_placeholder(value: &SecretValue, format: TemplateFormat) -> Option<String> {
    format.escape_text(value.expose())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value(text: &str) -> SecretValue {
        SecretValue::new(text.to_owned()).unwrap()
    }

    #[test]
    fn sha256_hex_matches_the_standard_test_vectors() {
        assert_eq!(
            hash(HashAlgo::Sha256Hex, &value("abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            hash(
                HashAlgo::Sha256Hex,
                &value("abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq")
            ),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
        assert_eq!(HashAlgo::Sha256Hex.as_code(), "sha256_hex");
    }

    #[test]
    fn stdin_and_env_carry_the_exact_value() {
        let secret = value("fixture value, never a real one");

        assert_eq!(stdin_payload(&secret), b"fixture value, never a real one");
        assert_eq!(
            env_pair("FAKE_TOKEN", &secret),
            (
                OsStr::new("FAKE_TOKEN"),
                OsStr::new("fixture value, never a real one")
            )
        );
    }

    #[test]
    fn a_placeholder_renders_with_the_escaper_of_its_format() {
        let secret = value("a\"b");

        assert_eq!(
            render_secret_placeholder(&secret, TemplateFormat::Json).as_deref(),
            Some("\"a\\\"b\"")
        );
        assert_eq!(
            render_secret_placeholder(&secret, TemplateFormat::Yaml).as_deref(),
            Some("\"a\\\"b\"")
        );
        assert_eq!(
            render_secret_placeholder(&secret, TemplateFormat::NatsConf),
            None
        );
        assert_eq!(
            render_secret_placeholder(&value("plain"), TemplateFormat::NatsConf).as_deref(),
            Some("\"plain\"")
        );
    }
}
