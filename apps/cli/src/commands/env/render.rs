//! Text that `validate`, `plan`, `import`, and `lock` share: the finding
//! lines, the counts, the plan table, the blocker lines, and the plan
//! sections that go to stderr. Every value comes from the daemon and is
//! escaped before it is written.

use stackly_protocol::env::{
    EnvFinding, EnvPlan, EnvValidation, PlanAction, PlanBlocker, PlanEndpoint, PlanItem, PlanLocal,
    PlanLock, PlanStep, RuntimeSource,
};
use stackly_protocol::instance::PortRequest;
use stackly_protocol::provider::FindingSeverity;

use crate::commands::path::home_masked;
use crate::commands::provider::local::esc;
use crate::output::human::{Cell, HumanWriter, wire_name};
use crate::session::human_hints;

/// Error and warning findings of the whole reply: the findings sent,
/// split by severity, plus the `omitted` ones, which count as errors
/// because every manifest code is an error (spec 0044, *Aturan validasi*;
/// the one warning of spec 0045 belongs to the local override file).
/// `findingCount` only raises the errors when it says there were more than
/// the reply accounts for.
#[must_use]
pub fn counts(validation: &EnvValidation) -> (u64, u64) {
    let sent = validation.findings.len() as u64;
    let warnings = validation
        .findings
        .iter()
        .filter(|finding| finding.severity == FindingSeverity::Warning)
        .count() as u64;
    let errors = (sent - warnings).saturating_add(u64::from(validation.omitted));
    let total = u64::from(validation.finding_count);
    (errors.max(total.saturating_sub(warnings)), warnings)
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

/// The `SOURCE` cell of an item (spec 0045 *CLI*): `lock` or `catalog`
/// for a runtime, `-` without one.
fn source_text(item: &PlanItem) -> Option<String> {
    item.runtime.as_ref().map(|runtime| match runtime.source {
        RuntimeSource::Lock => "lock".to_owned(),
        RuntimeSource::Catalog => "catalog".to_owned(),
        other => wire_name(&other),
    })
}

/// The rows of one item: one per step, or one `keep` row without steps.
fn item_rows(item: &PlanItem) -> Vec<Vec<Cell>> {
    let endpoints: Vec<String> = item.endpoints.iter().map(endpoint_text).collect();
    let source = source_text(item);
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
            Cell::optional(source.as_deref()),
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
                "source",
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

/// One `blocked: [<service>: ]<code>: <message>` line per blocker, then
/// its hints: the daemon's, then the CLI's own for the code (such as the
/// location and `--relink` of `env.name_taken`), whose wording also
/// replaces the message.
pub fn blocker_lines(out: &mut HumanWriter, blockers: &[PlanBlocker]) {
    for blocker in blockers {
        let service = blocker
            .service
            .as_deref()
            .map(|service| format!("{}: ", esc(service)))
            .unwrap_or_default();
        let extra = human_hints(&blocker.error, None);
        out.line(&format!(
            "blocked: {service}{}: {}",
            esc(blocker.error.code()),
            esc(extra.message.as_deref().unwrap_or(blocker.error.message()))
        ));
        for hint in blocker
            .error
            .hint()
            .into_iter()
            .chain(extra.hints.iter().map(String::as_str))
        {
            out.line(&format!("  hint: {}", esc(hint)));
        }
    }
}

/// `lock: <file> (<state>[, <targets>])`, then one line per alias that
/// does not match (spec 0045 *CLI*). Nothing from a daemon that sends no
/// lock (an empty file name).
pub fn lock_lines(out: &mut HumanWriter, lock: &PlanLock) {
    if lock.file.is_empty() {
        return;
    }
    let mut state = wire_name(&lock.state);
    if !lock.targets.is_empty() {
        let targets: Vec<String> = lock.targets.iter().map(|target| esc(target)).collect();
        state = format!("{state}, {}", targets.join(", "));
    }
    out.line(&format!("lock: {} ({state})", esc(&lock.file)));
    for item in &lock.stale {
        out.line(&format!(
            "  {}: {}",
            esc(&item.service),
            wire_name(&item.reason)
        ));
    }
}

/// `local: <file>`, one `<service>.<field>: <value> (<state>)` line per
/// override, then the findings of the file (spec 0045 *CLI*).
pub fn local_lines(out: &mut HumanWriter, local: &PlanLocal) {
    out.line(&format!("local: {}", esc(&local.file)));
    for item in &local.overrides {
        out.line(&format!(
            "  {}.{}: {} ({})",
            esc(&item.service),
            esc(&item.field),
            esc(&item.value),
            wire_name(&item.state)
        ));
    }
    for finding in &local.findings {
        out.line(&format!("  {}", finding_line(&local.file, finding)));
    }
}

/// The sections for stderr: the `lock:` and `local:` lines, then
/// `blocked:`, `secrets:`, `differs:`, and `not in manifest:`, each left
/// out when it is empty. A manifest that is not valid gets no `lock:` or
/// `local:` line: its lock state was never checked against the manifest
/// (spec 0045 K23), so only the manifest findings are shown.
pub fn plan_sections(out: &mut HumanWriter, plan: &EnvPlan) {
    if plan.validation.valid {
        lock_lines(out, &plan.lock);
        if let Some(local) = &plan.local {
            local_lines(out, local);
        }
    }
    blocker_lines(out, &plan.blockers);
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
    fn counts_add_the_omitted_findings_to_the_errors_and_keep_the_warnings_sent() {
        let finding = |severity| EnvFinding {
            severity,
            code: "x".to_owned(),
            location: "services".to_owned(),
            service: None,
            message: "m".to_owned(),
        };
        let mut validation = plan("plan-applicable.json").validation;
        validation.findings = vec![
            finding(FindingSeverity::Error),
            finding(FindingSeverity::Warning),
            finding(FindingSeverity::Error),
        ];
        validation.omitted = stackly_protocol::Count::saturating(4);
        validation.finding_count = stackly_protocol::Count::saturating(7);
        assert_eq!(counts(&validation), (6, 1));
        validation.finding_count = stackly_protocol::Count::saturating(3);
        assert_eq!(counts(&validation), (6, 1));
        validation.omitted = stackly_protocol::Count::saturating(0);
        validation.finding_count = stackly_protocol::Count::saturating(9);
        assert_eq!(counts(&validation), (8, 1));
    }

    #[test]
    fn the_lock_line_names_the_state_and_targets_and_each_stale_alias() {
        let lock = plan("plan-locked-blocked.json").lock;
        let mut out = HumanWriter::new(false, jiff::Timestamp::UNIX_EPOCH, jiff::tz::TimeZone::UTC);
        lock_lines(&mut out, &lock);
        assert_eq!(
            out.into_string(),
            "lock: stackly.lock (stale, macos-aarch64)\n  queue: not_locked\n"
        );
        let mut out = HumanWriter::new(false, jiff::Timestamp::UNIX_EPOCH, jiff::tz::TimeZone::UTC);
        lock_lines(&mut out, &PlanLock::default());
        assert_eq!(out.into_string(), "");
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
