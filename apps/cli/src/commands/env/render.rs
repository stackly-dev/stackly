//! Text that `validate`, `plan`, and `import` share: the finding lines,
//! the counts, the plan table, and the plan sections that go to stderr.
//! Every value comes from the daemon and is escaped before it is written.

use stackly_protocol::env::{
    EnvFinding, EnvPlan, EnvValidation, PlanAction, PlanEndpoint, PlanItem, PlanStep,
};
use stackly_protocol::instance::PortRequest;
use stackly_protocol::provider::FindingSeverity;

use crate::commands::path::home_masked;
use crate::commands::provider::local::esc;
use crate::output::human::{Cell, HumanWriter, wire_name};

/// Error and warning findings, counting the findings left out as errors
/// (only a manifest that is not valid has that many).
#[must_use]
pub fn counts(validation: &EnvValidation) -> (u64, u64) {
    let warnings = validation
        .findings
        .iter()
        .filter(|finding| finding.severity == FindingSeverity::Warning)
        .count() as u64;
    let total = u64::from(validation.finding_count);
    (total.saturating_sub(warnings), warnings)
}

/// `<file>: <location>: <code>: <message>`, with `warning: ` in front of a
/// warning.
#[must_use]
pub fn finding_line(file: &str, finding: &EnvFinding) -> String {
    let line = format!(
        "{}: {}: {}: {}",
        esc(file),
        esc(&finding.location),
        esc(&finding.code),
        esc(&finding.message)
    );
    if finding.severity == FindingSeverity::Warning {
        format!("warning: {line}")
    } else {
        line
    }
}

/// The finding lines of `validation`, errors first, then the line for
/// findings left out, and, for a manifest that is not valid, the count.
pub fn findings(out: &mut HumanWriter, validation: &EnvValidation) {
    let (errors, warnings) = counts(validation);
    let file = home_masked(&validation.file, std::env::home_dir().as_deref());
    let ordered = validation
        .findings
        .iter()
        .filter(|finding| finding.severity != FindingSeverity::Warning)
        .chain(
            validation
                .findings
                .iter()
                .filter(|finding| finding.severity == FindingSeverity::Warning),
        );
    for finding in ordered {
        out.line(&finding_line(&file, finding));
    }
    let omitted = u64::from(validation.omitted);
    if omitted > 0 {
        out.line(&format!("{omitted} more finding(s) not shown"));
    }
    if !validation.valid {
        out.line(&format!("{errors} error(s), {warnings} warning(s)"));
    }
}

/// The word of a step in the `ACTION` column.
fn step_word(step: PlanStep) -> String {
    match step {
        PlanStep::InstallRuntime => "install".to_owned(),
        PlanStep::CreateInstance => "create".to_owned(),
        PlanStep::StartInstance => "start".to_owned(),
        other => wire_name(&other),
    }
}

/// `<name>=<host>:<port>`, the port `auto` when Stackly picks it.
fn endpoint_text(endpoint: &PlanEndpoint) -> String {
    let port = match endpoint.port {
        PortRequest::Fixed(port) => port.to_string(),
        PortRequest::Auto => "auto".to_owned(),
    };
    format!("{}={}:{port}", endpoint.name, endpoint.host)
}

/// The rows of one item: one per step, or one `keep` row without steps.
fn item_rows(item: &PlanItem) -> Vec<Vec<Cell>> {
    let endpoints: Vec<String> = item.endpoints.iter().map(endpoint_text).collect();
    let row = |action: String| {
        vec![
            Cell::fixed(action),
            Cell::text(&item.alias),
            Cell::text(&item.instance.name),
            Cell::text(&item.provider),
            Cell::optional(
                item.runtime
                    .as_ref()
                    .map(|runtime| runtime.version.as_str()),
            ),
            Cell::text(&endpoints.join(", ")),
        ]
    };
    if item.steps.is_empty() {
        let action = match item.action {
            PlanAction::Keep => "keep".to_owned(),
            other => wire_name(&other),
        };
        return vec![row(action)];
    }
    item.steps
        .iter()
        .map(|step| row(step_word(*step)))
        .collect()
}

/// The counts of the summary line: runtimes to install, instances to
/// create, starts (those of `create` items included), and items kept.
#[must_use]
pub fn plan_counts(plan: &EnvPlan) -> [usize; 4] {
    let mut counts = [0; 4];
    for item in &plan.items {
        for step in &item.steps {
            match step {
                PlanStep::InstallRuntime => counts[0] += 1,
                PlanStep::CreateInstance => counts[1] += 1,
                PlanStep::StartInstance => counts[2] += 1,
                _ => {}
            }
        }
        if item.steps.is_empty() && item.action == PlanAction::Keep {
            counts[3] += 1;
        }
    }
    counts
}

/// The table, a blank line, and the summary line, for stdout.
pub fn plan_table(out: &mut HumanWriter, plan: &EnvPlan) {
    if !plan.items.is_empty() {
        let rows: Vec<Vec<Cell>> = plan.items.iter().flat_map(item_rows).collect();
        out.table(
            &[
                "action",
                "service",
                "instance",
                "provider",
                "version",
                "endpoints",
            ],
            &rows,
        );
        out.line("");
    }
    let [install, create, start, keep] = plan_counts(plan);
    out.line(&format!(
        "Plan: {install} to install, {create} to create, {start} to start, {keep} to keep."
    ));
}

/// The sections for stderr: `blocked:`, `secrets:`, `differs:`, and
/// `not in manifest:`, each left out when it is empty.
pub fn plan_sections(out: &mut HumanWriter, plan: &EnvPlan) {
    for blocker in &plan.blockers {
        let service = blocker
            .service
            .as_deref()
            .map(|service| format!("{}: ", esc(service)))
            .unwrap_or_default();
        out.line(&format!(
            "blocked: {service}{}: {}",
            esc(blocker.error.code()),
            esc(blocker.error.message())
        ));
        if let Some(hint) = blocker.error.hint() {
            out.line(&format!("  hint: {}", esc(hint)));
        }
    }
    let secrets: Vec<String> = plan
        .items
        .iter()
        .flat_map(|item| {
            item.secrets.iter().map(move |secret| {
                format!(
                    "  {} {} {}: {}",
                    esc(&item.alias),
                    esc(&secret.field),
                    secret
                        .reference
                        .as_deref()
                        .map_or_else(|| "-".to_owned(), esc),
                    wire_name(&secret.status)
                )
            })
        })
        .collect();
    section(out, "secrets:", &secrets);
    let differences: Vec<String> = plan
        .items
        .iter()
        .flat_map(|item| {
            item.differences.iter().map(move |difference| {
                format!(
                    "  {} {}: manifest {}, instance {}",
                    esc(&item.alias),
                    esc(&difference.field),
                    esc(&difference.manifest),
                    esc(&difference.instance)
                )
            })
        })
        .collect();
    section(out, "differs:", &differences);
    let detached: Vec<String> = plan
        .detached
        .iter()
        .map(|member| {
            format!(
                "  {} {} ({})",
                esc(&member.alias),
                esc(&member.instance.name),
                wire_name(&member.ownership)
            )
        })
        .collect();
    section(out, "not in manifest:", &detached);
}

/// `heading` and `lines`, or nothing when `lines` is empty.
fn section(out: &mut HumanWriter, heading: &str, lines: &[String]) {
    if lines.is_empty() {
        return;
    }
    out.line(heading);
    for line in lines {
        out.line(line);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan(name: &str) -> EnvPlan {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../protocols/fixtures/env")
            .join(name);
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn the_summary_counts_every_start_including_those_of_create_items() {
        let plan = plan("plan-applicable.json");
        let expected_starts = plan
            .items
            .iter()
            .flat_map(|item| &item.steps)
            .filter(|step| **step == PlanStep::StartInstance)
            .count();
        let [_, create, start, _] = plan_counts(&plan);
        assert_eq!(start, expected_starts);
        assert!(start >= create);
    }

    #[test]
    fn a_warning_line_starts_with_warning_and_an_error_line_does_not() {
        let mut finding = EnvFinding {
            severity: FindingSeverity::Error,
            code: "dependency_cycle".to_owned(),
            location: "services.api.dependsOn".to_owned(),
            service: Some("api".to_owned()),
            message: "dependency cycle: api -> db -> api".to_owned(),
        };
        assert_eq!(
            finding_line("/w/stackly.yaml", &finding),
            "/w/stackly.yaml: services.api.dependsOn: dependency_cycle: dependency cycle: api -> db -> api"
        );
        finding.severity = FindingSeverity::Warning;
        assert!(finding_line("/w/stackly.yaml", &finding).starts_with("warning: /w/"));
    }
}
