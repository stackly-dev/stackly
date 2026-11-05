//! The `traefik-routes` probe (spec 0009 AC-22, *Probe `traefik-routes`*):
//! the pure part of one attempt, without IO.
//!
//! [`expected_routers`] reads the routes file Stackly rendered into the
//! routers Traefik must have loaded, and [`compare`] checks the reply of
//! `GET /api/http/routers` against them. The supervisor (spec 0010,
//! `supervisor::probe::native`) reads the file, sends both requests, and
//! holds the deadline.

use std::collections::BTreeMap;

use serde::Deserialize;
use stackly_domain::ReasonCode;

use super::NativeProbeFailure;

/// The path of the liveness check, answered on the same loopback entry
/// point as the API.
pub const PING_PATH: &str = "/ping";
/// The path that lists the HTTP routers of every provider, in one page.
pub const ROUTERS_PATH: &str = "/api/http/routers?per_page=1000";
/// The most bytes of the routers reply that are read.
pub const ROUTERS_BODY_MAX: usize = 1 << 20;
/// The most bytes of the routes file that are read; a rendered file is
/// cut at 1 MiB (spec 0009, *Template config*).
pub const ROUTES_FILE_MAX: u64 = 1 << 20;
/// The most characters of a router name a detail quotes.
pub const DETAIL_NAME_MAX: usize = 64;
/// The provider name of the routers the file provider loaded.
pub const FILE_PROVIDER: &str = "file";

/// The reply could not be read, or the routes file could not.
pub const PROTOCOL_ERROR: ReasonCode = ReasonCode::from_static("protocol_error");
/// Traefik serves another set of routers than the routes file holds.
pub const ROUTES_MISMATCH: ReasonCode = ReasonCode::from_static("routes_mismatch");

/// The routers Traefik must have loaded: name to rule.
pub type Expected = BTreeMap<String, String>;

/// The failure when the routes file cannot be read or parsed.
#[must_use]
pub fn routes_file_unreadable() -> NativeProbeFailure {
    NativeProbeFailure::new(PROTOCOL_ERROR, "routes file is unreadable")
}

/// The failure for a Unix socket target, which validation already refuses
/// for `traefik-routes`; the attempt never connects.
#[must_use]
pub fn unix_target() -> NativeProbeFailure {
    NativeProbeFailure::new(PROTOCOL_ERROR, "traefik-routes supports tcp endpoints only")
}

/// Reads the routes file into the routers Traefik must serve. `None` is a
/// file that does not exist.
///
/// A missing file, an empty file, a file without `http`, and `http`
/// without `routers` or with an empty `routers` all expect no router.
///
/// # Errors
///
/// Returns a `protocol_error` failure when the file is not YAML, or when
/// `http.routers` is not a map of names to routers with a text `rule`.
pub fn expected_routers(file: Option<&[u8]>) -> Result<Expected, NativeProbeFailure> {
    use serde_yaml_ng::Value;

    let Some(bytes) = file else {
        return Ok(Expected::new());
    };
    if bytes.iter().all(u8::is_ascii_whitespace) {
        return Ok(Expected::new());
    }
    let document: Value = serde_yaml_ng::from_slice(bytes).map_err(|_| routes_file_unreadable())?;
    let routers = match &document {
        Value::Null => return Ok(Expected::new()),
        Value::Mapping(root) => match root.get("http") {
            None | Some(Value::Null) => return Ok(Expected::new()),
            Some(Value::Mapping(http)) => match http.get("routers") {
                None | Some(Value::Null) => return Ok(Expected::new()),
                Some(Value::Mapping(routers)) => routers,
                Some(_) => return Err(routes_file_unreadable()),
            },
            Some(_) => return Err(routes_file_unreadable()),
        },
        _ => return Err(routes_file_unreadable()),
    };
    let mut expected = Expected::new();
    for (name, router) in routers {
        let (Value::String(name), Value::Mapping(router)) = (name, router) else {
            return Err(routes_file_unreadable());
        };
        let Some(Value::String(rule)) = router.get("rule") else {
            return Err(routes_file_unreadable());
        };
        expected.insert(name.clone(), rule.clone());
    }
    Ok(expected)
}

/// One router of the API reply. Other fields are ignored.
#[derive(Debug, Deserialize)]
struct Router {
    name: String,
    provider: String,
    rule: String,
    status: String,
}

/// Checks the body of `GET /api/http/routers` against `expected`.
///
/// Only routers of the `file` provider count, with `@file` taken off their
/// name. The probe is ready when those names are exactly the expected
/// names, every rule is the expected rule byte for byte, and every status
/// is `enabled`.
///
/// # Errors
///
/// Returns a `protocol_error` failure when the body is not a JSON array
/// of objects with text `name`, `provider`, `rule`, and `status`, and a
/// `routes_mismatch` failure naming the first missing, extra, changed, or
/// not enabled router, in that order, without any rule text.
pub fn compare(expected: &Expected, body: &[u8]) -> Result<(), NativeProbeFailure> {
    let routers: Vec<Router> = serde_json::from_slice(body).map_err(|_| {
        NativeProbeFailure::new(PROTOCOL_ERROR, "the routers reply is not a list of routers")
    })?;
    let mut loaded: BTreeMap<String, Router> = BTreeMap::new();
    for router in routers {
        if router.provider != FILE_PROVIDER {
            continue;
        }
        let name = router
            .name
            .strip_suffix("@file")
            .unwrap_or(&router.name)
            .to_owned();
        loaded.insert(name, router);
    }
    let mismatch = |detail: String| Err(NativeProbeFailure::new(ROUTES_MISMATCH, detail));
    if let Some(name) = expected.keys().find(|name| !loaded.contains_key(*name)) {
        return mismatch(format!("router {} is not loaded", quoted(name)));
    }
    if let Some(name) = loaded.keys().find(|name| !expected.contains_key(*name)) {
        return mismatch(format!(
            "router {} is loaded but not in the routes file",
            quoted(name)
        ));
    }
    if let Some((name, _)) = expected.iter().find(|(name, rule)| {
        loaded
            .get(*name)
            .is_some_and(|router| router.rule != **rule)
    }) {
        return mismatch(format!(
            "router {} has another rule than the routes file",
            quoted(name)
        ));
    }
    if let Some((name, router)) = loaded.iter().find(|(_, router)| router.status != "enabled") {
        return mismatch(format!(
            "router {} is {}",
            quoted(name),
            quoted(&router.status)
        ));
    }
    Ok(())
}

/// A name from the server, safe for a detail: control characters become
/// `?`, and a name longer than [`DETAIL_NAME_MAX`] characters keeps its
/// first 61 and ends in `...`, all in double quotes.
fn quoted(text: &str) -> String {
    let cleaned: String = text
        .chars()
        .map(|c| if c.is_control() { '?' } else { c })
        .collect();
    if cleaned.chars().count() <= DETAIL_NAME_MAX {
        return format!("\"{cleaned}\"");
    }
    let cut: String = cleaned.chars().take(DETAIL_NAME_MAX - 3).collect();
    format!("\"{cut}...\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROUTES: &str = "# Written by Stackly from the routes setting; edits are replaced.
http:
  routers:
    \"api\":
      rule: \"Host(`api.localhost`)\"
      service: \"api\"
      entryPoints:
        - web
    \"docs\":
      rule: \"Host(\\\"docs.localhost\\\") || PathPrefix(`/docs`)\"
      service: \"docs\"
      entryPoints:
        - web
  services:
    \"api\":
      loadBalancer:
        servers:
          - url: \"http://127.0.0.1:3000\"
";

    fn expected() -> Expected {
        expected_routers(Some(ROUTES.as_bytes())).unwrap()
    }

    /// A routers reply the way Traefik 3.7 writes it, with its internal
    /// routers first.
    fn reply(file: &[(&str, &str, &str)]) -> String {
        let mut routers = vec![
            serde_json::json!({ "entryPoints": ["ping"], "service": "api@internal", "rule": "PathPrefix(`/api`)", "ruleSyntax": "default", "priority": 9223372036854775806_i64, "status": "enabled", "using": ["ping"], "name": "api@internal", "provider": "internal" }),
            serde_json::json!({ "entryPoints": ["ping"], "service": "ping@internal", "rule": "PathPrefix(`/ping`)", "status": "enabled", "name": "ping@internal", "provider": "internal" }),
        ];
        for (name, rule, status) in file {
            routers.push(serde_json::json!({
                "entryPoints": ["web"],
                "service": name,
                "rule": rule,
                "status": status,
                "using": ["web"],
                "name": format!("{name}@file"),
                "provider": "file",
            }));
        }
        serde_json::Value::Array(routers).to_string()
    }

    fn failure(result: Result<(), NativeProbeFailure>) -> (String, String) {
        let failure = result.expect_err("the comparison fails");
        (failure.reason.as_str().to_owned(), failure.detail)
    }

    #[test]
    fn the_routes_file_becomes_names_and_rules() {
        assert_eq!(
            expected(),
            Expected::from([
                ("api".to_owned(), "Host(`api.localhost`)".to_owned()),
                (
                    "docs".to_owned(),
                    "Host(\"docs.localhost\") || PathPrefix(`/docs`)".to_owned()
                ),
            ])
        );
    }

    // covers: AC-22
    #[test]
    fn the_same_routers_are_ready() {
        let body = reply(&[
            (
                "docs",
                "Host(\"docs.localhost\") || PathPrefix(`/docs`)",
                "enabled",
            ),
            ("api", "Host(`api.localhost`)", "enabled"),
        ]);

        assert_eq!(compare(&expected(), body.as_bytes()), Ok(()));
    }

    // covers: AC-22
    #[test]
    fn a_missing_router_is_a_routes_mismatch_naming_it() {
        let body = reply(&[("api", "Host(`api.localhost`)", "enabled")]);

        assert_eq!(
            failure(compare(&expected(), body.as_bytes())),
            (
                "routes_mismatch".to_owned(),
                "router \"docs\" is not loaded".to_owned()
            )
        );
    }

    // covers: AC-22
    #[test]
    fn an_extra_router_is_a_routes_mismatch_naming_it() {
        let body = reply(&[
            ("api", "Host(`api.localhost`)", "enabled"),
            (
                "docs",
                "Host(\"docs.localhost\") || PathPrefix(`/docs`)",
                "enabled",
            ),
            ("old", "Host(`old.localhost`)", "enabled"),
        ]);

        assert_eq!(
            failure(compare(&expected(), body.as_bytes())),
            (
                "routes_mismatch".to_owned(),
                "router \"old\" is loaded but not in the routes file".to_owned()
            )
        );
    }

    // covers: AC-22
    #[test]
    fn a_changed_rule_is_a_routes_mismatch_without_the_rule_text() {
        let body = reply(&[
            ("api", "Host(`api.localhost`)", "enabled"),
            ("docs", "Host(`docs.localhost`)", "enabled"),
        ]);

        let (reason, detail) = failure(compare(&expected(), body.as_bytes()));

        assert_eq!(reason, "routes_mismatch");
        assert_eq!(
            detail,
            "router \"docs\" has another rule than the routes file"
        );
        assert!(!detail.contains("localhost"), "{detail}");
    }

    // covers: AC-22
    #[test]
    fn a_disabled_router_is_a_routes_mismatch_with_its_status() {
        let body = reply(&[
            ("api", "Host(`api.localhost`)", "disabled"),
            (
                "docs",
                "Host(\"docs.localhost\") || PathPrefix(`/docs`)",
                "enabled",
            ),
        ]);

        assert_eq!(
            failure(compare(&expected(), body.as_bytes())),
            (
                "routes_mismatch".to_owned(),
                "router \"api\" is \"disabled\"".to_owned()
            )
        );
    }

    #[test]
    fn a_router_with_a_warning_is_not_ready() {
        let body = reply(&[
            ("api", "Host(`api.localhost`)", "enabled"),
            (
                "docs",
                "Host(\"docs.localhost\") || PathPrefix(`/docs`)",
                "warning",
            ),
        ]);

        assert_eq!(
            failure(compare(&expected(), body.as_bytes())).0,
            "routes_mismatch"
        );
    }

    #[test]
    fn routers_of_other_providers_are_ignored() {
        let body = reply(&[]);

        assert_eq!(compare(&Expected::new(), body.as_bytes()), Ok(()));
    }

    // covers: AC-22
    #[test]
    fn a_reply_that_is_not_a_list_of_routers_is_a_protocol_error() {
        for body in [
            "{\"name\":\"api@file\"}",
            "[{\"name\":\"api@file\",\"provider\":\"file\",\"status\":\"enabled\"}]",
            "[{\"name\":1,\"provider\":\"file\",\"rule\":\"x\",\"status\":\"enabled\"}]",
            "[\"api@file\"]",
            "not json",
            "",
        ] {
            assert_eq!(
                failure(compare(&expected(), body.as_bytes())),
                (
                    "protocol_error".to_owned(),
                    "the routers reply is not a list of routers".to_owned()
                ),
                "{body}"
            );
        }
    }

    // covers: AC-22
    #[test]
    fn a_missing_or_empty_routes_file_expects_no_router() {
        assert_eq!(expected_routers(None), Ok(Expected::new()));
        for file in [
            "",
            "  \n",
            "# only a comment\n",
            "http: {}\n",
            "http:\n  routers: {}\n",
            "http:\n  services: {}\n",
        ] {
            assert_eq!(
                expected_routers(Some(file.as_bytes())),
                Ok(Expected::new()),
                "{file:?}"
            );
        }
    }

    #[test]
    fn an_empty_expectation_fails_when_a_file_router_is_still_loaded() {
        let body = reply(&[("api", "Host(`api.localhost`)", "enabled")]);

        assert_eq!(
            failure(compare(&Expected::new(), body.as_bytes())).0,
            "routes_mismatch"
        );
    }

    // covers: AC-22
    #[test]
    fn a_routes_file_that_is_not_routers_is_unreadable() {
        for file in [
            "http: [\n",
            "- a list\n",
            "http: text\n",
            "http:\n  routers: [api]\n",
            "http:\n  routers:\n    api: {}\n",
            "http:\n  routers:\n    api:\n      rule: 7\n",
        ] {
            assert_eq!(
                expected_routers(Some(file.as_bytes())),
                Err(routes_file_unreadable()),
                "{file}"
            );
        }
    }

    #[test]
    fn a_name_in_a_detail_is_cut_and_has_no_control_characters() {
        let long = format!("a\u{7}{}", "b".repeat(100));
        let quoted = quoted(&long);

        assert!(quoted.starts_with("\"a?b"), "{quoted}");
        assert!(quoted.ends_with("...\""), "{quoted}");
        assert_eq!(quoted.chars().count(), DETAIL_NAME_MAX + 2);
    }
}
