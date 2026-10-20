//! Value parsers for command arguments (spec 0008, *Struktur perintah dan
//! flag global*). clap calls them, so a refused value exits 2 before
//! anything reaches the daemon.

use std::time::Duration;

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
}
