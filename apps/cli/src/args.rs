//! Value parsers for command arguments (spec 0008, *Struktur perintah dan
//! flag global*). clap calls them, so a refused value exits 2 before
//! anything reaches the daemon.

#![expect(
    dead_code,
    reason = "the instance commands use the new parsers in the next commit"
)]

use std::str::FromStr;
use std::time::Duration;

use stackly_protocol::instance::PortRequest;
use stackly_protocol::json::is_idempotency_key;
use stackly_protocol::operation::OperationCursor;
use stackly_protocol::secret::{SECRET_REFERENCE_MAX, reference_field_code};
use stackly_protocol::{ResourceKind, ResourceRef, Revision};
use uuid::Uuid;

/// The shortest `--timeout`.
pub const TIMEOUT_MIN: Duration = Duration::from_secs(1);
/// The longest `--timeout`, 168 hours.
pub const TIMEOUT_MAX: Duration = Duration::from_secs(168 * 3_600);

/// An operation ID: a whole UUID in its hyphenated form, never a prefix.
///
/// # Errors
///
/// Returns why `text` is not one, for clap's message.
pub fn operation_id(text: &str) -> Result<Uuid, String> {
    if text.len() != 36 {
        return Err(
            "expected a whole operation id, such as 0192f3a0-7c1e-7b2a-9d4e-5f6a7b8c9d0e"
                .to_owned(),
        );
    }
    Uuid::try_parse(text)
        .map_err(|_| "expected a whole operation id in hex with hyphens".to_owned())
}

/// `--resource`: `manager`, or `<kind>:<uuid>` with a kind the operation
/// list filter takes (`runtime`, `instance`, `environment`).
///
/// # Errors
///
/// Returns why `text` is not one, for clap's message.
pub fn resource_filter(text: &str) -> Result<ResourceRef, String> {
    if text == "manager" {
        return Ok(ResourceRef {
            kind: ResourceKind::Manager,
            id: "self".to_owned(),
        });
    }
    let refused =
        || "expected manager, or runtime, instance, or environment then :<uuid>".to_owned();
    let (kind, id) = text.split_once(':').ok_or_else(refused)?;
    let kind = match kind {
        "runtime" => ResourceKind::Runtime,
        "instance" => ResourceKind::Instance,
        "environment" => ResourceKind::Environment,
        _ => return Err(refused()),
    };
    let id = operation_id(id).map_err(|_| refused())?;
    Ok(ResourceRef {
        kind,
        id: id.to_string(),
    })
}

/// `--before`: a cursor exactly as a previous page's `next` wrote it.
///
/// # Errors
///
/// Returns why `text` is not one, for clap's message.
pub fn cursor(text: &str) -> Result<String, String> {
    OperationCursor::parse(text)
        .map(|_| text.to_owned())
        .ok_or_else(|| "expected the next value of an earlier page".to_owned())
}

/// `--timeout`: whole seconds (`90`), or jiff's friendly format with the
/// units `h`, `m`, and `s` (`30s`, `5m`, `1h30m`), from 1 second to 168
/// hours.
///
/// # Errors
///
/// Returns why `text` is not one, for clap's message.
pub fn timeout(text: &str) -> Result<Duration, String> {
    let refused = || {
        "expected seconds (90) or hours, minutes, and seconds (30s, 5m, 1h30m) from 1s to 168h"
            .to_owned()
    };
    if text.is_empty()
        || !text
            .bytes()
            .all(|b| b.is_ascii_digit() || matches!(b, b'h' | b'm' | b's'))
    {
        return Err(refused());
    }
    let duration = if text.bytes().all(|b| b.is_ascii_digit()) {
        Duration::from_secs(text.parse().map_err(|_| refused())?)
    } else {
        let parsed: jiff::SignedDuration = text.parse().map_err(|_| refused())?;
        Duration::try_from(parsed).map_err(|_| refused())?
    };
    if duration.subsec_nanos() != 0 || !(TIMEOUT_MIN..=TIMEOUT_MAX).contains(&duration) {
        return Err(refused());
    }
    Ok(duration)
}

/// A secret reference such as `local:instance/0192f3a0/password`: the
/// grammar of the protocol, so a malformed one never reaches the daemon.
///
/// # Errors
///
/// Returns why `text` is not one, for clap's message.
pub fn secret_reference(text: &str) -> Result<String, String> {
    match reference_field_code(text) {
        None => Ok(text.to_owned()),
        Some(_) if text.len() > SECRET_REFERENCE_MAX => Err(format!(
            "a secret reference is at most {SECRET_REFERENCE_MAX} bytes"
        )),
        Some(_) => Err(
            "expected local: then 2 to 8 segments of a-z, 0-9, dot, underscore, or hyphen, joined by /"
                .to_owned(),
        ),
    }
}

/// `--expected-revision`: a whole number the wire accepts.
///
/// # Errors
///
/// Returns why `text` is not one, for clap's message.
pub fn revision(text: &str) -> Result<Revision, String> {
    let refused = || "expected a whole number such as 3".to_owned();
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return Err(refused());
    }
    text.parse::<u64>()
        .ok()
        .and_then(Revision::new)
        .ok_or_else(refused)
}

/// `--idempotency-key`: 1 to 128 bytes of printable ASCII without spaces,
/// the rule of the protocol, so a key the daemon would refuse never leaves
/// the CLI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdempotencyKeyArg(String);

impl IdempotencyKeyArg {
    /// The key as it goes on the wire.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for IdempotencyKeyArg {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, String> {
        if is_idempotency_key(text) {
            Ok(Self(text.to_owned()))
        } else {
            Err("expected 1 to 128 printable ASCII characters without spaces".to_owned())
        }
    }
}

/// An instance ID argument: a whole UUID in its hyphenated form, in upper
/// or lower case, and never a prefix or a name (names arrive with the
/// resolver of spec 0022).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstanceIdArg(Uuid);

impl InstanceIdArg {
    /// The ID; its text form is always lower case.
    #[must_use]
    pub const fn get(self) -> Uuid {
        self.0
    }
}

impl FromStr for InstanceIdArg {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, String> {
        if text.len() != 36 {
            return Err(
                "expected a whole instance id, such as 0192f3a0-7c1e-7b2a-9d4e-5f6a7b8c9d0e"
                    .to_owned(),
            );
        }
        Uuid::try_parse(text)
            .map(Self)
            .map_err(|_| "expected a whole instance id in hex with hyphens".to_owned())
    }
}

/// `--port <endpoint>=<port|auto>`: the shape only. The number is 0 to
/// 65535 and 0 is still sent, because the daemon decides what a port may be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortArg {
    /// The endpoint name from the manifest.
    pub endpoint: String,
    /// What it asks for.
    pub port: PortRequest,
}

impl FromStr for PortArg {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, String> {
        let refused =
            || "expected <endpoint>=<port> or <endpoint>=auto, port 0 to 65535".to_owned();
        let (endpoint, port) = text.split_once('=').ok_or_else(refused)?;
        if endpoint.is_empty() {
            return Err(refused());
        }
        let port = if port == "auto" {
            PortRequest::Auto
        } else if !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) {
            PortRequest::Fixed(port.parse().map_err(|_| refused())?)
        } else {
            return Err(refused());
        };
        Ok(Self {
            endpoint: endpoint.to_owned(),
            port,
        })
    }
}

/// `--set <field>=<text>`: a settings field with a text value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingArg {
    /// The settings field.
    pub field: String,
    /// Its value, which may be empty.
    pub value: String,
}

impl FromStr for SettingArg {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, String> {
        let (field, value) = setting_parts(text)?;
        Ok(Self {
            field: field.to_owned(),
            value: value.to_owned(),
        })
    }
}

/// `--set-json <field>=<json>`: a settings field with any JSON value.
#[derive(Debug, Clone, PartialEq)]
pub struct SettingJsonArg {
    /// The settings field.
    pub field: String,
    /// Its value.
    pub value: serde_json::Value,
}

impl FromStr for SettingJsonArg {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, String> {
        let (field, value) = setting_parts(text)?;
        let value = serde_json::from_str(value)
            .map_err(|_| "expected <field>=<json>, and the value is not JSON".to_owned())?;
        Ok(Self {
            field: field.to_owned(),
            value,
        })
    }
}

/// `<field>=<value>` split at the first `=`, with a field that is not
/// empty.
fn setting_parts(text: &str) -> Result<(&str, &str), String> {
    match text.split_once('=') {
        Some((field, value)) if !field.is_empty() => Ok((field, value)),
        _ => Err("expected <field>=<value>".to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "0192f3a0-7c1e-7b2a-9d4e-5f6a7b8c9d0e";

    #[test]
    fn an_operation_id_is_a_whole_hyphenated_uuid() {
        assert_eq!(operation_id(ID), Ok(ID.parse().unwrap()));
        assert_eq!(
            operation_id("0192F3A0-7C1E-7B2A-9D4E-5F6A7B8C9D0E"),
            Ok(ID.parse().unwrap())
        );
        for text in [
            "0192f3a0",
            "0192f3a07c1e7b2a9d4e5f6a7b8c9d0e",
            "{0192f3a0-7c1e-7b2a-9d4e-5f6a7b8c9d0e}",
            "urn:uuid:0192f3a0-7c1e-7b2a-9d4e-5f6a7b8c9d0e",
            "0192f3a0-7c1e-7b2a-9d4e-5f6a7b8c9d0g",
            "",
        ] {
            assert!(operation_id(text).is_err(), "{text}");
        }
    }

    #[test]
    fn a_resource_is_manager_or_a_kind_with_an_operation_and_a_uuid() {
        assert_eq!(
            resource_filter("manager"),
            Ok(ResourceRef {
                kind: ResourceKind::Manager,
                id: "self".to_owned()
            })
        );
        for (kind, expected) in [
            ("runtime", ResourceKind::Runtime),
            ("instance", ResourceKind::Instance),
            ("environment", ResourceKind::Environment),
        ] {
            assert_eq!(
                resource_filter(&format!("{kind}:{ID}")),
                Ok(ResourceRef {
                    kind: expected,
                    id: ID.to_owned()
                })
            );
        }
        for text in [
            "operation:0192f3a0-7c1e-7b2a-9d4e-5f6a7b8c9d0e",
            "instance",
            "instance:abc",
            "manager:self",
            "Instance:0192f3a0-7c1e-7b2a-9d4e-5f6a7b8c9d0e",
        ] {
            assert!(resource_filter(text).is_err(), "{text}");
        }
    }

    #[test]
    fn a_cursor_must_be_one_the_daemon_wrote() {
        let next = format!("2026-10-18T03:04:05.000000000Z_{ID}");
        assert_eq!(cursor(&next), Ok(next.clone()));
        for text in ["", "abc", &format!("2026-10-18T03:04:05Z_{ID}")] {
            assert!(cursor(text).is_err(), "{text}");
        }
    }

    #[test]
    fn a_timeout_is_whole_seconds_or_h_m_s_from_one_second_to_168_hours() {
        for (text, seconds) in [
            ("90", 90),
            ("1", 1),
            ("30s", 30),
            ("5m", 300),
            ("1h30m", 5_400),
            ("2h30m5s", 9_005),
            ("168h", 604_800),
        ] {
            assert_eq!(timeout(text), Ok(Duration::from_secs(seconds)), "{text}");
        }
        for text in [
            "0",
            "0s",
            "169h",
            "604801",
            "1.5h",
            "5ms",
            "1d",
            "PT5M",
            "-5s",
            "1h 30m",
            "1h30",
            "",
            "abc",
            "99999999999999999999",
        ] {
            assert!(timeout(text).is_err(), "{text}");
        }
    }

    #[test]
    fn a_secret_reference_follows_the_protocol_grammar() {
        for text in [
            "local:user/api-token",
            "local:instance/0192f3a0-7c1e-7b2a-9d4e-5f6a7b8c9d0e/password",
        ] {
            assert_eq!(secret_reference(text), Ok(text.to_owned()), "{text}");
        }
        for text in [
            "",
            "local:",
            "local:one",
            "Local:user/x",
            "local:User/x",
            "local:user//x",
            "local:user/../x",
            "keychain:user/x",
            &format!("local:user/{}", "a".repeat(300)),
        ] {
            assert!(secret_reference(text).is_err(), "{text}");
        }
    }

    #[test]
    fn a_revision_is_a_whole_number_the_wire_accepts() {
        assert_eq!(revision("0"), Ok(Revision::new(0).unwrap()));
        assert_eq!(revision("42"), Ok(Revision::new(42).unwrap()));
        for text in [
            "",
            "-1",
            "1.5",
            "abc",
            "+1",
            "99999999999999999999",
            "9007199254740992",
        ] {
            assert!(revision(text).is_err(), "{text}");
        }
    }

    #[test]
    fn an_idempotency_key_is_one_to_128_printable_ascii_bytes() {
        for text in ["k", "create-orders-db-0001", &"a".repeat(128), "!~"] {
            assert_eq!(
                text.parse::<IdempotencyKeyArg>().unwrap().as_str(),
                text,
                "{text}"
            );
        }
        for text in ["", " ", "a b", "tab\t", "é", &"a".repeat(129)] {
            assert!(text.parse::<IdempotencyKeyArg>().is_err(), "{text}");
        }
    }

    #[test]
    fn an_instance_id_is_a_whole_hyphenated_uuid_in_either_case() {
        let lower: InstanceIdArg = ID.parse().unwrap();
        let upper: InstanceIdArg = ID.to_uppercase().parse().unwrap();
        assert_eq!(lower, upper);
        assert_eq!(lower.get().to_string(), ID);
        for text in [
            "",
            "0192f3a0",
            "orders-db",
            "0192f3a07c1e7b2a9d4e5f6a7b8c9d0e",
            "{0192f3a0-7c1e-7b2a-9d4e-5f6a7b8c9d0e}",
            "0192f3a0-7c1e-7b2a-9d4e-5f6a7b8c9d0g",
        ] {
            assert!(text.parse::<InstanceIdArg>().is_err(), "{text}");
        }
    }

    #[test]
    fn a_port_is_an_endpoint_with_auto_or_a_number_up_to_65535() {
        let port = |text: &str| text.parse::<PortArg>();
        assert_eq!(
            port("postgres=5432"),
            Ok(PortArg {
                endpoint: "postgres".to_owned(),
                port: PortRequest::Fixed(5432)
            })
        );
        assert_eq!(port("admin=auto").unwrap().port, PortRequest::Auto);
        assert_eq!(port("x=0").unwrap().port, PortRequest::Fixed(0));
        assert_eq!(port("x=65535").unwrap().port, PortRequest::Fixed(65535));
        for text in [
            "", "x", "=80", "x=", "x=65536", "x=-1", "x=+1", "x=1.5", "x=Auto", "x=80 ", "x=0x50",
        ] {
            assert!(port(text).is_err(), "{text}");
        }
    }

    #[test]
    fn a_setting_splits_at_the_first_equals_and_keeps_the_rest_as_it_is() {
        assert_eq!(
            "token=a=b".parse::<SettingArg>(),
            Ok(SettingArg {
                field: "token".to_owned(),
                value: "a=b".to_owned()
            })
        );
        assert_eq!("note=".parse::<SettingArg>().unwrap().value, "");
        for text in ["", "x", "=v"] {
            assert!(text.parse::<SettingArg>().is_err(), "{text}");
            assert!(text.parse::<SettingJsonArg>().is_err(), "{text}");
        }
    }

    #[test]
    fn a_json_setting_takes_any_json_value_and_refuses_text_that_is_not() {
        let json = |text: &str| text.parse::<SettingJsonArg>().map(|arg| arg.value);
        assert_eq!(json("n=1500"), Ok(serde_json::json!(1500)));
        assert_eq!(json("f=true"), Ok(serde_json::json!(true)));
        assert_eq!(json("s=\"x\""), Ok(serde_json::json!("x")));
        assert_eq!(
            json("o={\"a\":[1,null]}"),
            Ok(serde_json::json!({"a": [1, null]}))
        );
        for text in ["n=", "n=abc", "n={", "n=1 2"] {
            assert!(json(text).is_err(), "{text}");
        }
    }
}
