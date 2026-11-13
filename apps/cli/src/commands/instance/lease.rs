//! The words of an environment lease on an instance (spec 0047, *CLI*,
//! AC-19): the lines and hints a refused stop or port change ends with,
//! and the line a stop with `--override-leases` adds for each environment
//! that is no longer complete.

use stackly_protocol::codes::{instance as instance_codes, supervisor};
use stackly_protocol::env::InstanceEnvironment;

use crate::commands::provider::local::{esc, shows_text};
use crate::context::Context;
use crate::failure::CliFailure;

/// Whether `failure` is a refusal an override answers:
/// `supervisor.stop_requires_override` or
/// `instance.endpoints_require_override`.
#[must_use]
pub fn needs_override(failure: &CliFailure) -> bool {
    failure.body.is(&supervisor::STOP_REQUIRES_OVERRIDE)
        || failure.body.is(&instance_codes::ENDPOINTS_REQUIRE_OVERRIDE)
}

/// The environments the `environment` items of `failure` name, in order,
/// each once.
fn holders(failure: &CliFailure) -> Vec<(String, Option<String>)> {
    let mut seen: Vec<(String, Option<String>)> = Vec::new();
    for item in failure.body.items() {
        if item.kind() != "environment" {
            continue;
        }
        let Some(name) = item.field("name").or_else(|| item.field("environmentId")) else {
            continue;
        };
        if seen.iter().any(|(known, _)| known == name) {
            continue;
        }
        let alias = item
            .field("alias")
            .filter(|alias| !alias.is_empty())
            .map(str::to_owned);
        seen.push((name.to_owned(), alias));
    }
    seen
}

/// One `<instance> is used by <environment> (<alias>)` line per
/// environment the refusal names.
#[must_use]
pub fn held_lines(failure: &CliFailure, instance: &str) -> Vec<String> {
    holders(failure)
        .into_iter()
        .map(|(environment, alias)| match alias {
            Some(alias) => format!(
                "{} is used by {} ({})",
                esc(instance),
                esc(&environment),
                esc(&alias)
            ),
            None => format!("{} is used by {}", esc(instance), esc(&environment)),
        })
        .collect()
}

/// `failure` with the hints of a refusal: `retry` again with
/// `--override-leases`, or `stackly env down` for each environment.
#[must_use]
pub fn with_hints(mut failure: CliFailure, retry: &str) -> CliFailure {
    let environments = holders(&failure);
    failure.hints.push(format!(
        "to go ahead anyway, run: {retry} --override-leases"
    ));
    for (environment, _) in environments {
        failure.hints.push(format!(
            "or bring the environment down first: stackly env down {}",
            esc(&environment)
        ));
    }
    failure
}

/// Writes the lines of a refusal to stderr, unless the output is JSON or
/// quiet, and adds its hints; any other failure comes back unchanged.
#[must_use]
pub fn explain(cx: &mut Context, failure: CliFailure, instance: &str, retry: &str) -> CliFailure {
    if !needs_override(&failure) {
        return failure;
    }
    if shows_text(cx, false) {
        for line in held_lines(&failure, instance) {
            cx.output.stderr.line(&line);
        }
    }
    with_hints(failure, retry)
}

/// One `<name> stopped. <environment> is no longer complete.` line per
/// environment whose lease is still active on the stopped instance.
#[must_use]
pub fn incomplete_lines(name: &str, environments: &[InstanceEnvironment]) -> Vec<String> {
    environments
        .iter()
        .filter(|environment| environment.lease_active)
        .map(|environment| {
            format!(
                "{} stopped. {} is no longer complete.",
                esc(name),
                esc(&environment.name)
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use stackly_protocol::ErrorBody;
    use stackly_protocol::env::EnvOwnership;
    use stackly_protocol::error::ErrorItem;
    use uuid::Uuid;

    use super::*;

    fn refusal() -> CliFailure {
        let item = |name: &str, alias: &str| {
            ErrorItem::new("environment")
                .with_field("environmentId", Uuid::from_u128(1).to_string())
                .with_field("name", name)
                .with_field("alias", alias)
                .with_field("ownership", "shared")
        };
        CliFailure::daemon(
            ErrorBody::new(&supervisor::STOP_REQUIRES_OVERRIDE, "held")
                .with_items(vec![item("erp", "db"), item("reporting", "")]),
            None,
        )
    }

    #[test]
    fn a_refusal_names_each_environment_with_its_alias() {
        assert_eq!(
            held_lines(&refusal(), "orders-db"),
            [
                "orders-db is used by erp (db)",
                "orders-db is used by reporting"
            ]
        );
    }

    #[test]
    fn the_hints_name_the_flag_and_env_down_for_each_environment() {
        let failure = with_hints(refusal(), "stackly instance stop orders-db");
        assert_eq!(
            failure.hints,
            [
                "to go ahead anyway, run: stackly instance stop orders-db --override-leases",
                "or bring the environment down first: stackly env down erp",
                "or bring the environment down first: stackly env down reporting",
            ]
        );
    }

    #[test]
    fn only_environments_with_an_active_lease_are_no_longer_complete() {
        let environment = |name: &str, lease_active: bool| InstanceEnvironment {
            environment_id: Uuid::from_u128(1),
            name: name.to_owned(),
            alias: "db".to_owned(),
            ownership: EnvOwnership::Shared,
            lease_active,
        };
        assert_eq!(
            incomplete_lines(
                "orders-db",
                &[environment("erp", true), environment("old", false)]
            ),
            ["orders-db stopped. erp is no longer complete."]
        );
    }
}
