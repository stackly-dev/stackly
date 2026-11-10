//! The text of `stackly doctor` (spec 0034, *CLI*): one block per check,
//! the client checks first and then the daemon's, Errors before Warnings
//! before OK. A check without findings is one line with its summary; a
//! finding is its title, then its explanation, evidence, output excerpt,
//! remedy, and `Next:` command indented under it. The last line counts the
//! errors and warnings.
//!
//! On a colored stdout the marks are `✓`, `!`, and `✗` in the tone of
//! their severity; without color they are the words `OK`, `WARNING`, and
//! `ERROR` (spec 0008, *Output manusia*). Every text from the daemon is
//! escaped.

use stackly_protocol::doctor::{
    DoctorAction, DoctorActionKind, DoctorCheck, DoctorCheckId, DoctorEvidence, DoctorFinding,
    DoctorRemedy, DoctorSeverity, ProcessRef,
};
use stackly_protocol::instance::IdentityCheckResult;
use unicode_width::UnicodeWidthStr;
use uuid::Uuid;

use super::DoctorResult;
use super::local::next_step;
use crate::output::escape::escape_display;
use crate::output::human::{HumanWriter, Render, wire_name};
use crate::output::style::Tone;

/// The widest mark without color, `WARNING`.
const WORD_WIDTH: usize = 7;

impl Render for DoctorResult {
    fn render(&self, out: &mut HumanWriter) {
        let report_checks: Vec<&DoctorCheck> = self
            .report
            .iter()
            .flat_map(|report| by_severity(&report.checks))
            .collect();
        let label_width = self
            .local
            .iter()
            .chain(report_checks.iter().copied())
            .map(|check| escape_display(&check.title).width())
            .max()
            .unwrap_or(0);
        let layout = Layout {
            label_width,
            mark_width: if out.colored() { 1 } else { WORD_WIDTH },
        };
        for check in &self.local {
            write_check(out, check, &layout);
        }
        if !report_checks.is_empty() {
            out.line("");
            for check in report_checks {
                write_check(out, check, &layout);
            }
        }
        out.line("");
        out.line(&self.footer());
    }
}

impl DoctorResult {
    /// The last line: the errors and warnings, how many checks ran, and the
    /// run ID of the daemon's report.
    fn footer(&self) -> String {
        let errors = self.count(DoctorSeverity::Error);
        let warnings = self.count(DoctorSeverity::Warning);
        let issues = if errors == 0 && warnings == 0 {
            "No issues".to_owned()
        } else {
            format!(
                "{}, {}",
                plural(errors, "error", "errors"),
                plural(warnings, "warning", "warnings")
            )
        };
        let checks =
            self.local.len() + self.report.as_ref().map_or(0, |report| report.checks.len());
        let mut line = format!("{issues} · {}", plural(checks as u64, "check", "checks"));
        if let Some(report) = &self.report {
            line.push_str(&format!(" · run {}", report.run_id));
        }
        line
    }
}

/// `n` with the singular or plural noun.
fn plural(n: u64, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// The checks of a report, Errors first, then Warnings, then anything this
/// build does not know, then OK, each group in report order.
fn by_severity(checks: &[DoctorCheck]) -> Vec<&DoctorCheck> {
    let mut sorted: Vec<&DoctorCheck> = checks.iter().collect();
    sorted.sort_by_key(|check| rank(check.severity));
    sorted
}

const fn rank(severity: DoctorSeverity) -> u8 {
    match severity {
        DoctorSeverity::Error => 0,
        DoctorSeverity::Warning => 1,
        DoctorSeverity::Unrecognized => 2,
        DoctorSeverity::Ok => 3,
    }
}

/// Where the columns are.
struct Layout {
    label_width: usize,
    mark_width: usize,
}

impl Layout {
    /// The indent of the lines under a finding: under its title.
    const fn indent(&self) -> usize {
        self.label_width + 2 + self.mark_width + 1
    }
}

/// One check: a line with its summary, or one block per finding.
fn write_check(out: &mut HumanWriter, check: &DoctorCheck, layout: &Layout) {
    let label = escape_display(&check.title).into_owned();
    if check.findings.is_empty() {
        let line = head(out, &label, check.severity, &check.summary, layout);
        out.line(&line);
        if check.omitted.get() > 0 {
            write_body(out, layout, &[more(check.omitted.get())]);
        }
        return;
    }
    for (index, finding) in check.findings.iter().enumerate() {
        let label = if index == 0 { label.as_str() } else { "" };
        let line = head(out, label, finding.severity, &finding.title, layout);
        out.line(&line);
        write_body(out, layout, &body(out, check.id, finding));
    }
    if check.omitted.get() > 0 {
        write_body(out, layout, &[more(check.omitted.get())]);
    }
}

/// `… and n more`, for findings past the limit.
fn more(n: u64) -> String {
    format!("… and {n} more")
}

/// The first line of a check or a finding: the label, the mark, and the
/// escaped `text`.
fn head(
    out: &HumanWriter,
    label: &str,
    severity: DoctorSeverity,
    text: &str,
    layout: &Layout,
) -> String {
    let mut line = label.to_owned();
    line.push_str(&" ".repeat(layout.label_width - label.width() + 2));
    let (symbol, word, tone) = match severity {
        DoctorSeverity::Error => ("✗", "ERROR", Tone::Critical),
        DoctorSeverity::Warning => ("!", "WARNING", Tone::Caution),
        DoctorSeverity::Ok => ("✓", "OK", Tone::Positive),
        DoctorSeverity::Unrecognized => ("?", "UNKNOWN", Tone::Neutral),
    };
    if out.colored() {
        line.push_str(&paint(symbol, tone));
    } else {
        line.push_str(word);
        line.push_str(&" ".repeat(WORD_WIDTH - word.len()));
    }
    line.push(' ');
    line.push_str(&escape_display(text));
    line
}

/// `text` in the color of `tone`, through the same style the result
/// tables use.
fn paint(text: &str, tone: Tone) -> String {
    let style = tone.style();
    format!("{}{text}{}", style.render(), style.render_reset())
}

/// Writes `lines` under a finding, each already escaped.
fn write_body(out: &mut HumanWriter, layout: &Layout, lines: &[String]) {
    let indent = " ".repeat(layout.indent());
    for line in lines {
        out.line(&format!("{indent}{line}"));
    }
}

/// The lines under a finding, escaped: its explanation, the identity
/// evidence, the output excerpt, the remedy, and the `Next:` command.
fn body(out: &HumanWriter, check: DoctorCheckId, finding: &DoctorFinding) -> Vec<String> {
    let mut lines = Vec::new();
    if !finding.explanation.is_empty() {
        lines.push(escape_display(&finding.explanation).into_owned());
    }
    if let Some(evidence) = &finding.evidence {
        lines.extend(evidence_lines(out, evidence));
    }
    if !finding.excerpt.is_empty() {
        lines.push("Output:".to_owned());
        lines.extend(
            finding
                .excerpt
                .iter()
                .map(|line| format!("  {}", escape_display(line))),
        );
    }
    if let Some(remedy) = &finding.remedy {
        lines.extend(remedy_lines(remedy));
    }
    let next = if matches!(
        check,
        DoctorCheckId::Connection | DoctorCheckId::Protocol | DoctorCheckId::BackgroundAgent
    ) {
        next_step(finding)
    } else {
        finding
            .action
            .as_ref()
            .and_then(|action| action_command(finding, action))
    };
    if let Some(next) = next {
        lines.push(format!("Next: {}", escape_display(&next)));
    }
    lines
}

/// The recorded process, the process that holds the PID now, and the
/// identity checks that failed.
fn evidence_lines(out: &HumanWriter, evidence: &DoctorEvidence) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(recorded) = &evidence.recorded {
        lines.push(format!("Recorded: {}", process(out, recorded)));
    }
    if let Some(current) = &evidence.current {
        lines.push(format!("Now:      {}", process(out, current)));
    }
    let failed: Vec<String> = evidence
        .identity
        .checks
        .iter()
        .filter(|check| check.result == IdentityCheckResult::Failed)
        .map(|check| wire_name(&check.name))
        .collect();
    if !failed.is_empty() {
        lines.push(format!("Failed checks: {}", failed.join(", ")));
    }
    if evidence.identity.stopped {
        lines.push("The process is suspended.".to_owned());
    }
    lines
}

/// `PID 812 · started <time> · <executable>`, escaped.
fn process(out: &HumanWriter, process: &ProcessRef) -> String {
    let mut text = format!("PID {}", process.pid);
    if let Some(started) = process.started_at {
        text.push_str(&format!(" · started {}", out.time(started)));
    }
    match (&process.executable, process.same_user) {
        (Some(executable), _) => {
            text.push_str(&format!(" · {}", escape_display(executable)));
        }
        (None, Some(false)) => text.push_str(" · a process of another user"),
        (None, _) => {}
    }
    text
}

/// The remedy's title, then its numbered steps, each with the command and
/// the path it names on lines of their own.
fn remedy_lines(remedy: &DoctorRemedy) -> Vec<String> {
    let mut lines = vec![escape_display(&remedy.title).into_owned()];
    for (index, step) in remedy.steps.iter().enumerate() {
        let number = format!("{}. ", index + 1);
        let under = " ".repeat(number.len() + 2);
        lines.push(format!("  {number}{}", escape_display(&step.text)));
        for extra in [&step.command, &step.path].into_iter().flatten() {
            lines.push(format!("{under}{}", escape_display(extra)));
        }
    }
    lines
}

/// The command a safe action stands for (AC-18); `None` for an action this
/// build does not know or one that lacks what its command needs.
fn action_command(finding: &DoctorFinding, action: &DoctorAction) -> Option<String> {
    let instance = || instance_arg(finding, action.instance_id);
    match action.kind {
        DoctorActionKind::ShowInstance => Some(format!("stackly instance inspect {}", instance()?)),
        DoctorActionKind::ReviewConfig => {
            Some(format!("stackly instance config get {}", instance()?))
        }
        DoctorActionKind::ChangePort => Some(format!(
            "stackly instance endpoint set {} --port {}={}",
            instance()?,
            action.endpoint.as_deref()?,
            action.port?
        )),
        DoctorActionKind::ShowOperation => Some(format!(
            "stackly operation inspect {}",
            action.operation_id?
        )),
        DoctorActionKind::OpenStorageSettings => Some("stackly storage locations".to_owned()),
        DoctorActionKind::Unrecognized => None,
    }
}

/// How a command names the instance of an action: the finding's name when
/// it is the same instance, else the whole ID.
fn instance_arg(finding: &DoctorFinding, id: Option<Uuid>) -> Option<String> {
    match (&finding.instance, id) {
        (Some(instance), Some(id)) if instance.id == id => Some(instance.name.clone()),
        (_, Some(id)) => Some(id.to_string()),
        (Some(instance), None) => Some(instance.name.clone()),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use stackly_protocol::doctor::DoctorInstanceRef;

    use super::*;

    const INSTANCE: Uuid = Uuid::from_u128(0x0192_f3a4_0000_7000_8000_0000_0000_00b1);

    fn finding_with(action: DoctorAction) -> DoctorFinding {
        DoctorFinding {
            code: stackly_protocol::doctor::DoctorFindingCode::PortInUse,
            severity: DoctorSeverity::Error,
            title: "Port conflict · mysql-legacy".to_owned(),
            explanation: String::new(),
            subject: None,
            instance: Some(DoctorInstanceRef {
                id: INSTANCE,
                name: "mysql-legacy".to_owned(),
            }),
            operation_id: None,
            details: Vec::new(),
            evidence: None,
            port_owner: None,
            excerpt: Vec::new(),
            remedy: None,
            action: Some(action),
        }
    }

    fn action(kind: DoctorActionKind) -> DoctorAction {
        DoctorAction {
            kind,
            instance_id: Some(INSTANCE),
            operation_id: Some(Uuid::from_u128(7)),
            endpoint: Some("mysql".to_owned()),
            port: Some(3308),
        }
    }

    #[test]
    fn each_safe_action_is_written_as_the_command_it_stands_for() {
        for (kind, expected) in [
            (
                DoctorActionKind::ShowInstance,
                Some("stackly instance inspect mysql-legacy".to_owned()),
            ),
            (
                DoctorActionKind::ReviewConfig,
                Some("stackly instance config get mysql-legacy".to_owned()),
            ),
            (
                DoctorActionKind::ChangePort,
                Some("stackly instance endpoint set mysql-legacy --port mysql=3308".to_owned()),
            ),
            (
                DoctorActionKind::ShowOperation,
                Some(format!("stackly operation inspect {}", Uuid::from_u128(7))),
            ),
            (
                DoctorActionKind::OpenStorageSettings,
                Some("stackly storage locations".to_owned()),
            ),
            (DoctorActionKind::Unrecognized, None),
        ] {
            let finding = finding_with(action(kind));
            assert_eq!(
                action_command(&finding, &action(kind)),
                expected,
                "{kind:?}"
            );
        }
    }

    #[test]
    fn a_port_change_without_a_suggested_port_has_no_command() {
        let mut change = action(DoctorActionKind::ChangePort);
        change.port = None;

        assert_eq!(action_command(&finding_with(change.clone()), &change), None);
    }

    #[test]
    fn an_action_on_another_instance_names_its_whole_id() {
        let mut other = action(DoctorActionKind::ShowInstance);
        let id = Uuid::from_u128(9);
        other.instance_id = Some(id);

        assert_eq!(
            action_command(&finding_with(other.clone()), &other),
            Some(format!("stackly instance inspect {id}"))
        );
    }
}
