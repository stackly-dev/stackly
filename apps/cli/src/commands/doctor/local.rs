//! The client checks of `stackly doctor` (spec 0034, *Daftar check*, AC-11,
//! AC-15): `connection`, `protocol`, and `background_agent`. Only a client
//! sees them, so the CLI works them out itself and the daemon never sends
//! them. Each is a [`DoctorCheck`] of the same shape the daemon's checks
//! have, so `--json` holds one type.
//!
//! Nothing here runs `launchctl` or any other process, and nothing changes:
//! the agent state comes from the port's status call, the rest from the
//! connection attempt, the launch record and the record of a failed start
//! that the daemon wrote, and the modification time of the daemon binary.

use std::path::{Path, PathBuf};

use stackly_platform::bundle::DAEMON_BUNDLE_PROGRAM;
use stackly_platform::{AgentAssessment, BackgroundAgentState, UnavailableReason};
use stackly_protocol::codes::{ipc, protocol};
use stackly_protocol::doctor::{
    DOCTOR_DETAIL_VALUE_MAX, DOCTOR_EXPLANATION_MAX, DoctorCheck, DoctorCheckId, DoctorDetail,
    DoctorFinding, DoctorFindingCode, DoctorSeverity,
};
use stackly_protocol::handshake::HandshakeAccepted;
use stackly_protocol::{Count, ErrorBody, ProtocolVersion, Timestamp};

use crate::commands::daemon::agent::{AgentEnv, AgentReport, recovery_text};
use crate::commands::daemon::start_failure::StartFailureRecord;

/// How long after `lastLaunchedAt` the daemon binary may have been written
/// before it counts as replaced (spec 0034 AC-11).
pub const REPLACED_AFTER: jiff::SignedDuration = jiff::SignedDuration::from_secs(2);

/// The command that starts the daemon again.
pub const DAEMON_START: &str = "stackly daemon start";

/// How the connection attempt ended.
#[derive(Debug, Clone, Copy)]
pub enum Reach<'a> {
    /// The daemon accepted the handshake.
    Accepted(&'a HandshakeAccepted),
    /// The connection or the handshake failed with this error.
    Failed(&'a ErrorBody),
}

impl<'a> Reach<'a> {
    /// Whether a daemon answered on the socket: the handshake was accepted,
    /// or refused because the daemon speaks another protocol major.
    #[must_use]
    pub fn reached(self) -> bool {
        match self {
            Self::Accepted(_) => true,
            Self::Failed(body) => body.is(&protocol::MAJOR_MISMATCH),
        }
    }

    /// The error code when no daemon answered on the socket.
    #[must_use]
    pub fn unreachable_code(self) -> Option<&'a str> {
        match self {
            Self::Failed(body) if !self.reached() => Some(body.code()),
            _ => None,
        }
    }
}

/// Whether `code` says nothing listens at the socket: it is not there, or
/// it refuses connections.
#[must_use]
pub fn no_listener(code: &str) -> bool {
    code == ipc::ENDPOINT_MISSING.code || code == ipc::CONNECTION_REFUSED.code
}

/// The three client checks, in the order of the check table.
#[must_use]
pub fn checks(reach: Reach<'_>, socket: &Path, agent: &AgentFacts<'_>) -> Vec<DoctorCheck> {
    vec![
        connection(reach, socket),
        protocol(reach),
        background_agent(agent),
    ]
}

/// The `connection` check: OK with the daemon version, or one
/// `daemon_unreachable` Error whose `code` detail is the transport's error.
#[must_use]
pub fn connection(reach: Reach<'_>, socket: &Path) -> DoctorCheck {
    let socket_text = socket.display().to_string();
    let body = match reach {
        Reach::Accepted(accepted) => {
            return ok(
                DoctorCheckId::Connection,
                format!(
                    "Connected to stacklyd {} over the Unix socket",
                    accepted.daemon.version
                ),
            );
        }
        Reach::Failed(_) if reach.reached() => {
            return ok(
                DoctorCheckId::Connection,
                "Connected to stacklyd over the Unix socket".to_owned(),
            );
        }
        Reach::Failed(body) => body,
    };
    let explanation = if reach.unreachable_code().is_some_and(no_listener) {
        format!("No stacklyd answers at {socket_text}.")
    } else if body.is(&ipc::ENDPOINT_UNTRUSTED) {
        let reason = body.detail("reason").unwrap_or("untrusted");
        format!("Stackly doesn’t trust the socket at {socket_text} ({reason}).")
    } else if body.is(&ipc::CALL_TIMEOUT) {
        "stacklyd didn’t answer the handshake within 5 seconds.".to_owned()
    } else {
        format!("The connection failed: {}.", body.message())
    };
    with_findings(
        DoctorCheckId::Connection,
        "stacklyd isn’t reachable".to_owned(),
        vec![finding(
            DoctorFindingCode::DaemonUnreachable,
            DoctorSeverity::Error,
            "stacklyd isn’t reachable",
            explanation,
            vec![detail("code", body.code()), detail("path", &socket_text)],
        )],
    )
}

/// The `protocol` check: OK when the handshake was accepted, one
/// `protocol_incompatible` Error when the daemon speaks another major, and
/// one `check_unavailable` Warning when no daemon answered.
#[must_use]
pub fn protocol(reach: Reach<'_>) -> DoctorCheck {
    let client = ProtocolVersion::CURRENT;
    let client_text = format!("{}.{}", client.major, client.minor);
    match reach {
        Reach::Accepted(accepted) => {
            let daemon = accepted.protocol;
            let mut summary = if daemon == client {
                format!("App and daemon both speak protocol {client_text}")
            } else {
                format!(
                    "App speaks protocol {client_text} and daemon {}.{}; they work together",
                    daemon.major, daemon.minor
                )
            };
            if accepted.contract_drift {
                summary.push_str(" · their contract digests differ");
            }
            ok(DoctorCheckId::Protocol, summary)
        }
        Reach::Failed(body) if body.is(&protocol::MAJOR_MISMATCH) => {
            let daemon_text = match (body.detail("serverMajor"), body.detail("serverMinor")) {
                (Some(major), Some(minor)) => format!("{major}.{minor}"),
                (Some(major), None) => major.to_owned(),
                _ => "another major".to_owned(),
            };
            with_findings(
                DoctorCheckId::Protocol,
                "App and daemon speak different protocols".to_owned(),
                vec![finding(
                    DoctorFindingCode::ProtocolIncompatible,
                    DoctorSeverity::Error,
                    "Protocol mismatch",
                    format!(
                        "stackly speaks protocol {client_text} and stacklyd speaks {daemon_text}, so they can’t work together. Update Stackly so both match."
                    ),
                    vec![
                        detail("clientProtocol", &client_text),
                        detail("daemonProtocol", &daemon_text),
                    ],
                )],
            )
        }
        Reach::Failed(_) => with_findings(
            DoctorCheckId::Protocol,
            "Not checked while stacklyd isn’t reachable".to_owned(),
            vec![finding(
                DoctorFindingCode::CheckUnavailable,
                DoctorSeverity::Warning,
                "Protocol check unavailable",
                "The protocol is checked once stacklyd answers.".to_owned(),
                vec![detail("reason", "not_connected")],
            )],
        ),
    }
}

/// What the `background_agent` check is made from.
#[derive(Debug, Clone, Copy)]
pub struct AgentFacts<'a> {
    /// The state read from the port.
    pub assessment: &'a AgentAssessment,
    /// The report of that state, with `start` as the recovery of an
    /// `enabled` agent whose daemon is not reachable.
    pub report: &'a AgentReport,
    /// The error code when no daemon answered on the socket.
    pub unreachable: Option<&'a str>,
    /// Whether the launch record is there.
    pub launch_record: bool,
    /// `lastLaunchedAt` of the launch record.
    pub last_launched_at: Option<jiff::Timestamp>,
    /// The record of a failed start, read only while no daemon answers.
    pub start_failure: Option<&'a StartFailureRecord>,
    /// The bundle launchd starts the daemon from.
    pub agent_bundle: Option<&'a Path>,
    /// When `Contents/MacOS/stacklyd` of that bundle was last written.
    pub binary_modified_at: Option<jiff::Timestamp>,
}

/// The facts of the `background_agent` check from `agent` and how the
/// connection ended. It reads files only: the record of a failed start
/// while no daemon answers, and the daemon binary's metadata only for an
/// `enabled` agent whose daemon has no socket or refuses it.
#[must_use]
pub fn gather(agent: &AgentEnv, assessment: &AgentAssessment, reach: Reach<'_>) -> GatheredAgent {
    let report = agent.report(assessment, reach.reached());
    let unreachable = reach.unreachable_code();
    let start_failure = unreachable.and_then(|_| agent.start_failures().read());
    let records = agent.records();
    let agent_bundle = records
        .bundle_path()
        .or_else(|| agent.bundle_path().map(PathBuf::from));
    let binary_modified_at = (assessment.state == BackgroundAgentState::Enabled
        && unreachable.is_some_and(no_listener))
    .then(|| agent_bundle.as_deref().and_then(binary_modified_at))
    .flatten();
    GatheredAgent {
        assessment: *assessment,
        report,
        unreachable: unreachable.map(str::to_owned),
        launch_record: records.exists(),
        last_launched_at: records.last_launched_at(),
        start_failure,
        agent_bundle,
        binary_modified_at,
    }
}

/// The owned facts [`gather`] read, which [`GatheredAgent::facts`] lends
/// to [`background_agent`].
#[derive(Debug, Clone)]
pub struct GatheredAgent {
    assessment: AgentAssessment,
    report: AgentReport,
    unreachable: Option<String>,
    launch_record: bool,
    last_launched_at: Option<jiff::Timestamp>,
    start_failure: Option<StartFailureRecord>,
    agent_bundle: Option<PathBuf>,
    binary_modified_at: Option<jiff::Timestamp>,
}

impl GatheredAgent {
    /// The facts, borrowed.
    #[must_use]
    pub fn facts(&self) -> AgentFacts<'_> {
        AgentFacts {
            assessment: &self.assessment,
            report: &self.report,
            unreachable: self.unreachable.as_deref(),
            launch_record: self.launch_record,
            last_launched_at: self.last_launched_at,
            start_failure: self.start_failure.as_ref(),
            agent_bundle: self.agent_bundle.as_deref(),
            binary_modified_at: self.binary_modified_at,
        }
    }
}

/// When `<bundle>/Contents/MacOS/stacklyd` was last written, without
/// following a symlink in its place; `None` when it is not there.
#[must_use]
pub fn binary_modified_at(bundle: &Path) -> Option<jiff::Timestamp> {
    let modified = std::fs::symlink_metadata(bundle.join(DAEMON_BUNDLE_PROGRAM))
        .ok()?
        .modified()
        .ok()?;
    jiff::Timestamp::try_from(modified).ok()
}

/// The `background_agent` check: one `agent_state` finding for a state the
/// tone table of spec 0015 marks critical (Error) or caution (Warning), one
/// `daemon_start_failed` Error while no daemon answers and a record of a
/// failed start is there, and one `binary_replaced` Warning when launchd
/// may refuse a daemon binary written after it last started it.
#[must_use]
pub fn background_agent(facts: &AgentFacts<'_>) -> DoctorCheck {
    let mut findings = Vec::new();
    findings.extend(agent_state(facts));
    if facts.unreachable.is_some()
        && let Some(record) = facts.start_failure
    {
        findings.push(start_failed(record));
    }
    findings.extend(binary_replaced(facts));
    let summary = state_summary(facts.assessment);
    if findings.is_empty() {
        ok(DoctorCheckId::BackgroundAgent, summary)
    } else {
        with_findings(DoctorCheckId::BackgroundAgent, summary, findings)
    }
}

/// The summary of the agent state.
fn state_summary(assessment: &AgentAssessment) -> String {
    let text = match (assessment.state, assessment.reason) {
        (BackgroundAgentState::Enabled, _) => "Login item registered and allowed",
        (BackgroundAgentState::NotRegistered, _) => "Not registered",
        (BackgroundAgentState::RequiresApproval, _) => "Waiting for your approval in Login Items",
        (BackgroundAgentState::Disabled, _) => "Turned off in Login Items",
        (BackgroundAgentState::NotFound, _) => "macOS can’t find the app it belongs to",
        (BackgroundAgentState::Unavailable, Some(UnavailableReason::RootOverride)) => {
            "Not used while STACKLY_ROOT is set"
        }
        (BackgroundAgentState::Unavailable, Some(UnavailableReason::NotBundled)) => {
            "Only the stackly inside Stackly.app can read it"
        }
        (BackgroundAgentState::Unavailable, Some(UnavailableReason::HelperMissing)) => {
            "This Stackly.app has no stacklyd inside"
        }
        (BackgroundAgentState::Unavailable, Some(UnavailableReason::UnsupportedOs)) => {
            "Not available on this system"
        }
        (BackgroundAgentState::Unavailable, _) => "macOS couldn’t report its state",
    };
    text.to_owned()
}

/// The `agent_state` finding, for the states that need you.
fn agent_state(facts: &AgentFacts<'_>) -> Option<DoctorFinding> {
    let (severity, explanation) = match facts.assessment.state {
        BackgroundAgentState::NotRegistered => (
            DoctorSeverity::Warning,
            "The background agent isn’t registered, so stacklyd doesn’t start when you log in.",
        ),
        BackgroundAgentState::RequiresApproval => (
            DoctorSeverity::Warning,
            "macOS is waiting for you to allow Stackly in Login Items, so stacklyd doesn’t start when you log in.",
        ),
        BackgroundAgentState::Disabled => (
            DoctorSeverity::Warning,
            "Stackly is turned off in Login Items, so stacklyd doesn’t start when you log in.",
        ),
        BackgroundAgentState::NotFound => (
            DoctorSeverity::Error,
            "macOS can’t find the app the background agent was registered from, so it can’t start stacklyd.",
        ),
        BackgroundAgentState::Enabled | BackgroundAgentState::Unavailable => return None,
    };
    let mut details = vec![detail("state", facts.report.state)];
    if let Some(reason) = facts.report.reason {
        details.push(detail("reason", reason));
    }
    details.push(detail("recovery", facts.report.recovery));
    Some(finding(
        DoctorFindingCode::AgentState,
        severity,
        "Background agent needs you",
        explanation.to_owned(),
        details,
    ))
}

/// The `daemon_start_failed` finding of `record`.
fn start_failed(record: &StartFailureRecord) -> DoctorFinding {
    let mut details = vec![
        detail("at", &record.at.to_string()),
        detail("code", &record.code),
        detail("message", &record.message),
    ];
    if let Some(version) = &record.version {
        details.push(detail("version", version));
    }
    if let Some(launch) = &record.launch {
        details.push(detail("launch", launch));
    }
    finding(
        DoctorFindingCode::DaemonStartFailed,
        DoctorSeverity::Error,
        "stacklyd failed to start",
        format!(
            "The last start of stacklyd, at {}, failed: {} ({}).",
            record.at, record.message, record.code
        ),
        details,
    )
}

/// The `binary_replaced` finding: the agent is `enabled`, nothing is at
/// the socket or it refuses connections, the launch record is there, no
/// record of a failed start is newer than its `lastLaunchedAt`, and the
/// daemon binary was written more than [`REPLACED_AFTER`] after it.
fn binary_replaced(facts: &AgentFacts<'_>) -> Option<DoctorFinding> {
    if facts.assessment.state != BackgroundAgentState::Enabled
        || !facts.unreachable.is_some_and(no_listener)
        || !facts.launch_record
    {
        return None;
    }
    let launched = facts.last_launched_at?;
    if facts
        .start_failure
        .is_some_and(|record| record.at.as_jiff() > launched)
    {
        return None;
    }
    let modified = facts.binary_modified_at?;
    if modified.duration_since(launched) <= REPLACED_AFTER {
        return None;
    }
    let bundle = facts.agent_bundle.map_or_else(
        || "the app".to_owned(),
        |bundle| bundle.display().to_string(),
    );
    let launched = Timestamp::from_jiff(launched);
    let modified = Timestamp::from_jiff(modified);
    Some(finding(
        DoctorFindingCode::BinaryReplaced,
        DoctorSeverity::Warning,
        "stacklyd may have been replaced",
        format!(
            "stacklyd in {bundle} changed at {modified}, after launchd last started it at {launched}, so launchd may not start it again by itself."
        ),
        vec![
            detail("lastLaunchedAt", &launched.to_string()),
            detail("modifiedAt", &modified.to_string()),
            detail("bundlePath", &bundle),
        ],
    ))
}

/// The command or step the text writes as `Next:` under a client finding:
/// the recovery step of an agent state, and starting the daemon again for
/// a binary that may have been replaced.
#[must_use]
pub fn next_step(finding: &DoctorFinding) -> Option<String> {
    match finding.code {
        DoctorFindingCode::AgentState => finding
            .details
            .iter()
            .find(|detail| detail.key == "recovery")
            .and_then(|detail| recovery_text(&detail.value))
            .map(str::to_owned),
        DoctorFindingCode::BinaryReplaced => Some(DAEMON_START.to_owned()),
        _ => None,
    }
}

/// The title of a client check.
const fn title(check: DoctorCheckId) -> &'static str {
    match check {
        DoctorCheckId::Connection => "Connection",
        DoctorCheckId::Protocol => "Protocol",
        _ => "Background agent",
    }
}

/// A check without findings.
fn ok(id: DoctorCheckId, summary: String) -> DoctorCheck {
    with_findings(id, summary, Vec::new())
}

/// A check whose severity is the heaviest of `findings`.
fn with_findings(id: DoctorCheckId, summary: String, findings: Vec<DoctorFinding>) -> DoctorCheck {
    let severity = if findings
        .iter()
        .any(|finding| finding.severity == DoctorSeverity::Error)
    {
        DoctorSeverity::Error
    } else if findings.is_empty() {
        DoctorSeverity::Ok
    } else {
        DoctorSeverity::Warning
    };
    DoctorCheck {
        id,
        severity,
        title: title(id).to_owned(),
        summary,
        findings,
        omitted: Count::ZERO,
        capacity: None,
    }
}

/// A finding with only text and details.
fn finding(
    code: DoctorFindingCode,
    severity: DoctorSeverity,
    title: &str,
    explanation: String,
    details: Vec<DoctorDetail>,
) -> DoctorFinding {
    DoctorFinding {
        code,
        severity,
        title: title.to_owned(),
        explanation: clip_chars(&explanation, DOCTOR_EXPLANATION_MAX),
        subject: None,
        instance: None,
        operation_id: None,
        details,
        evidence: None,
        port_owner: None,
        excerpt: Vec::new(),
        remedy: None,
        action: None,
    }
}

/// A detail, its value cut to the contract's 512 bytes on a character
/// boundary.
fn detail(key: &str, value: &str) -> DoctorDetail {
    let mut end = value.len().min(DOCTOR_DETAIL_VALUE_MAX);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    DoctorDetail {
        key: key.to_owned(),
        value: value[..end].to_owned(),
    }
}

/// `text` cut to `max` characters.
fn clip_chars(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

#[cfg(test)]
mod tests {
    use stackly_platform::RecoveryStep;
    use stackly_protocol::ErrorBody;
    use stackly_protocol::codes::ipc;

    use super::*;

    fn assessment(
        state: BackgroundAgentState,
        reason: Option<UnavailableReason>,
    ) -> AgentAssessment {
        AgentAssessment {
            state,
            reason,
            recovery: RecoveryStep::None,
        }
    }

    fn report(state: &'static str, recovery: &'static str) -> AgentReport {
        AgentReport {
            state,
            reason: None,
            recovery,
            label: "com.ojiepermana.stackly.daemon",
            bundle_path: Some("/Applications/Stackly.app".to_owned()),
            last_launched_at: None,
        }
    }

    fn at(text: &str) -> jiff::Timestamp {
        text.parse().unwrap()
    }

    fn record(text: &str) -> StartFailureRecord {
        serde_json::from_value(serde_json::json!({
            "at": text,
            "code": "daemon.start_failed",
            "message": "the root override is writable by group or others",
            "version": "0.9.0",
            "launch": "launchd",
        }))
        .unwrap()
    }

    /// An `enabled` agent whose daemon has no socket, a launch record from
    /// 08:00:00, and a binary written at `modified`.
    fn replaced_facts<'a>(
        assessment: &'a AgentAssessment,
        report: &'a AgentReport,
        modified: &str,
        start_failure: Option<&'a StartFailureRecord>,
    ) -> AgentFacts<'a> {
        AgentFacts {
            assessment,
            report,
            unreachable: Some(ipc::ENDPOINT_MISSING.code),
            launch_record: true,
            last_launched_at: Some(at("2026-11-10T08:00:00Z")),
            start_failure,
            agent_bundle: Some(Path::new("/Applications/Stackly.app")),
            binary_modified_at: Some(at(modified)),
        }
    }

    fn codes(check: &DoctorCheck) -> Vec<DoctorFindingCode> {
        check.findings.iter().map(|finding| finding.code).collect()
    }

    #[test]
    fn a_daemon_that_is_not_running_is_one_connection_error_and_an_unchecked_protocol() {
        let body = ErrorBody::new(&ipc::ENDPOINT_MISSING, "nothing at the socket");
        let reach = Reach::Failed(&body);

        let connection = connection(reach, Path::new("/r/stacklyd.sock"));
        let protocol = protocol(reach);

        assert_eq!(connection.severity, DoctorSeverity::Error);
        assert_eq!(codes(&connection), [DoctorFindingCode::DaemonUnreachable]);
        let finding = &connection.findings[0];
        assert_eq!(
            finding.explanation,
            "No stacklyd answers at /r/stacklyd.sock."
        );
        assert_eq!(finding.details[0].value, "ipc.endpoint_missing");
        assert_eq!(protocol.severity, DoctorSeverity::Warning);
        assert_eq!(codes(&protocol), [DoctorFindingCode::CheckUnavailable]);
        assert_eq!(reach.unreachable_code(), Some("ipc.endpoint_missing"));
        assert!(!reach.reached());
    }

    #[test]
    fn a_daemon_of_another_major_is_reached_but_its_protocol_is_an_error() {
        let body = ErrorBody::new(
            &protocol::MAJOR_MISMATCH,
            "the client speaks another protocol major",
        )
        .with_detail("serverMajor", "9")
        .with_detail("serverMinor", "1");
        let reach = Reach::Failed(&body);

        let connection = connection(reach, Path::new("/r/stacklyd.sock"));
        let protocol = protocol(reach);

        assert_eq!(connection.severity, DoctorSeverity::Ok);
        assert!(connection.findings.is_empty());
        assert_eq!(codes(&protocol), [DoctorFindingCode::ProtocolIncompatible]);
        assert!(
            protocol.findings[0]
                .explanation
                .contains("stacklyd speaks 9.1")
        );
        assert_eq!(reach.unreachable_code(), None);
    }

    #[test]
    fn another_connection_error_names_its_message_and_counts_as_unreachable() {
        let body = ErrorBody::new(&ipc::CONNECTION_CLOSED, "the daemon closed the connection");
        let reach = Reach::Failed(&body);

        let check = connection(reach, Path::new("/r/stacklyd.sock"));

        assert_eq!(
            check.findings[0].explanation,
            "The connection failed: the daemon closed the connection."
        );
        assert_eq!(reach.unreachable_code(), Some("ipc.connection_closed"));
    }

    #[test]
    fn each_agent_state_gets_the_severity_of_its_spec_0015_tone() {
        for (state, code, expected) in [
            (BackgroundAgentState::Enabled, "enabled", None),
            (BackgroundAgentState::Unavailable, "unavailable", None),
            (
                BackgroundAgentState::NotRegistered,
                "not_registered",
                Some(DoctorSeverity::Warning),
            ),
            (
                BackgroundAgentState::RequiresApproval,
                "requires_approval",
                Some(DoctorSeverity::Warning),
            ),
            (
                BackgroundAgentState::Disabled,
                "disabled",
                Some(DoctorSeverity::Warning),
            ),
            (
                BackgroundAgentState::NotFound,
                "not_found",
                Some(DoctorSeverity::Error),
            ),
        ] {
            let assessment = assessment(state, None);
            let report = report(code, "register");
            let facts = AgentFacts {
                assessment: &assessment,
                report: &report,
                unreachable: None,
                launch_record: false,
                last_launched_at: None,
                start_failure: None,
                agent_bundle: None,
                binary_modified_at: None,
            };

            let check = background_agent(&facts);

            assert_eq!(
                check.findings.first().map(|finding| finding.severity),
                expected,
                "{code}"
            );
            if let Some(finding) = check.findings.first() {
                assert_eq!(finding.code, DoctorFindingCode::AgentState);
                assert_eq!(finding.details[0].value, code);
                assert_eq!(
                    next_step(finding).as_deref(),
                    Some("install it with stackly daemon install")
                );
            }
        }
    }

    #[test]
    fn a_start_failure_record_shows_only_while_no_daemon_answers() {
        let assessment = assessment(
            BackgroundAgentState::Unavailable,
            Some(UnavailableReason::RootOverride),
        );
        let report = report("unavailable", "manual_daemon_for_override");
        let record = record("2026-11-10T08:00:00Z");
        let mut facts = AgentFacts {
            assessment: &assessment,
            report: &report,
            unreachable: Some(ipc::ENDPOINT_MISSING.code),
            launch_record: false,
            last_launched_at: None,
            start_failure: Some(&record),
            agent_bundle: None,
            binary_modified_at: None,
        };

        let down = background_agent(&facts);
        facts.unreachable = None;
        let up = background_agent(&facts);

        assert_eq!(down.severity, DoctorSeverity::Error);
        assert_eq!(codes(&down), [DoctorFindingCode::DaemonStartFailed]);
        assert_eq!(
            down.findings[0].explanation,
            "The last start of stacklyd, at 2026-11-10T08:00:00.000Z, failed: the root override is writable by group or others (daemon.start_failed)."
        );
        assert_eq!(down.summary, "Not used while STACKLY_ROOT is set");
        assert_eq!(up.severity, DoctorSeverity::Ok);
        assert!(up.findings.is_empty());
    }

    #[test]
    fn a_binary_written_more_than_2_seconds_after_the_last_launch_may_be_replaced() {
        let assessment = assessment(BackgroundAgentState::Enabled, None);
        let report = report("enabled", "start");

        let replaced = background_agent(&replaced_facts(
            &assessment,
            &report,
            "2026-11-10T08:00:02.001Z",
            None,
        ));
        let same = background_agent(&replaced_facts(
            &assessment,
            &report,
            "2026-11-10T08:00:02Z",
            None,
        ));

        assert_eq!(replaced.severity, DoctorSeverity::Warning);
        assert_eq!(codes(&replaced), [DoctorFindingCode::BinaryReplaced]);
        assert!(
            replaced.findings[0]
                .explanation
                .contains("may not start it again")
        );
        assert_eq!(
            next_step(&replaced.findings[0]).as_deref(),
            Some("stackly daemon start")
        );
        assert!(same.findings.is_empty(), "{same:?}");
    }

    #[test]
    fn a_newer_start_failure_explains_the_stop_instead_of_a_replaced_binary() {
        let assessment = assessment(BackgroundAgentState::Enabled, None);
        let report = report("enabled", "start");
        let newer = record("2026-11-10T08:00:01Z");
        let older = record("2026-11-10T07:59:00Z");

        let with_newer = background_agent(&replaced_facts(
            &assessment,
            &report,
            "2026-11-10T09:00:00Z",
            Some(&newer),
        ));
        let with_older = background_agent(&replaced_facts(
            &assessment,
            &report,
            "2026-11-10T09:00:00Z",
            Some(&older),
        ));

        assert_eq!(codes(&with_newer), [DoctorFindingCode::DaemonStartFailed]);
        assert_eq!(
            codes(&with_older),
            [
                DoctorFindingCode::DaemonStartFailed,
                DoctorFindingCode::BinaryReplaced
            ]
        );
    }

    #[test]
    fn a_replaced_binary_needs_an_enabled_agent_a_missing_socket_and_a_launch_record() {
        let enabled = assessment(BackgroundAgentState::Enabled, None);
        let disabled = assessment(BackgroundAgentState::Disabled, None);
        let report = report("enabled", "start");
        let base = replaced_facts(&enabled, &report, "2026-11-10T09:00:00Z", None);

        let other_error = AgentFacts {
            unreachable: Some(ipc::CONNECTION_CLOSED.code),
            ..base
        };
        let no_record = AgentFacts {
            launch_record: false,
            ..base
        };
        let not_enabled = AgentFacts {
            assessment: &disabled,
            ..base
        };

        for facts in [other_error, no_record, not_enabled] {
            assert!(
                !codes(&background_agent(&facts)).contains(&DoctorFindingCode::BinaryReplaced),
                "{facts:?}"
            );
        }
        let refused = AgentFacts {
            unreachable: Some(ipc::CONNECTION_REFUSED.code),
            ..base
        };
        assert_eq!(
            codes(&background_agent(&refused)),
            [DoctorFindingCode::BinaryReplaced]
        );
    }

    #[test]
    fn the_modification_time_of_the_daemon_binary_is_read_without_following_a_symlink() {
        let dir = tempfile::tempdir().unwrap();
        let bundle = dir.path().join("Stackly.app");
        let macos = bundle.join("Contents/MacOS");
        std::fs::create_dir_all(&macos).unwrap();
        let binary = macos.join("stacklyd");
        std::fs::write(&binary, b"#!").unwrap();
        let when = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_800_000_000);
        std::fs::File::options()
            .write(true)
            .open(&binary)
            .unwrap()
            .set_modified(when)
            .unwrap();

        assert_eq!(
            binary_modified_at(&bundle),
            Some(jiff::Timestamp::from_second(1_800_000_000).unwrap())
        );
        assert_eq!(binary_modified_at(dir.path()), None);
    }

    #[test]
    fn a_detail_value_is_cut_to_512_bytes_on_a_character_boundary() {
        let value = "é".repeat(400);

        let cut = detail("message", &value);

        assert_eq!(cut.value.len(), 512);
        assert!(cut.value.chars().all(|c| c == 'é'));
    }
}
