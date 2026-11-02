//! Maps the facts of a config change to the DTOs of `stackly-protocol`
//! (spec 0029, *Method IPC*): the view of `instance.config.get`, the
//! verdicts of `instance.config.validate` and a dry run of
//! `instance.change_endpoints`, and the diff of a drift.

use std::collections::BTreeMap;

use stackly_core::instance::config::{
    ConfigFacts, ConfigImpact as CoreImpact, EffectiveFile as CoreFile, EndpointIssue,
    SettingChange as CoreChange,
};
use stackly_core::provider::{ApplyClass, SettingClass, SettingField};
use stackly_core::supervisor::drift::{DriftKind as CoreDriftKind, DriftReport};
use stackly_domain::{
    ConfigRevisionNumber as DomainNumber, ConfigRevisionOrigin as DomainOrigin,
    ConfigRevisionRecord, Endpoint, EndpointName, Instance, PortRequest as DomainPort, Transport,
};
use stackly_protocol::instance::config::{
    ConfigDrift, ConfigEndpoint, ConfigField, ConfigImpact, ConfigRevisionOrigin,
    ConfigRevisionSummary, DiffLine, DiffLineKind, DriftFile, DriftKind, EffectiveFile,
    EndpointChange, EndpointProblem, InstanceConfigView, SettingChange,
};
use stackly_protocol::instance::{
    EndpointRequest, EndpointTransport, PortRequest, SettingsDocument,
};
use stackly_protocol::{ConfigRevisionNumber, Timestamp};

use super::create::form_field;

/// The most bytes of one config file the reply carries; a longer file is
/// cut at a line end.
pub const FILE_BYTES_MAX: usize = 256 * 1024;
/// The most lines of one drift diff.
pub const DIFF_LINES_MAX: usize = 2_000;

/// The note when nothing can show the config files.
pub const RUNTIME_MISSING_NOTE: &str = "Runtime missing, preview unavailable";

/// A revision number on the wire.
#[must_use]
pub fn number(number: DomainNumber) -> ConfigRevisionNumber {
    ConfigRevisionNumber::saturating(number.get())
}

/// The impact on the wire.
#[must_use]
pub const fn impact(impact: CoreImpact) -> ConfigImpact {
    match impact {
        CoreImpact::None => ConfigImpact::None,
        CoreImpact::NextStart => ConfigImpact::NextStart,
        CoreImpact::Reload => ConfigImpact::Reload,
        CoreImpact::Restart => ConfigImpact::Restart,
    }
}

/// The wire text of an impact, for the journal input of an operation.
#[must_use]
pub const fn impact_text(impact: CoreImpact) -> &'static str {
    match impact {
        CoreImpact::None => "none",
        CoreImpact::NextStart => "next_start",
        CoreImpact::Reload => "reload",
        CoreImpact::Restart => "restart",
    }
}

/// The impact an operation recorded in its journal input; unknown when the
/// text is not one this build wrote.
#[must_use]
pub fn impact_of_text(text: &str) -> ConfigImpact {
    match text {
        "none" => ConfigImpact::None,
        "next_start" => ConfigImpact::NextStart,
        "reload" => ConfigImpact::Reload,
        "restart" => ConfigImpact::Restart,
        _ => ConfigImpact::Unrecognized,
    }
}

fn origin(origin: DomainOrigin) -> ConfigRevisionOrigin {
    match origin {
        DomainOrigin::Created => ConfigRevisionOrigin::Created,
        DomainOrigin::Applied => ConfigRevisionOrigin::Applied,
        DomainOrigin::Endpoints => ConfigRevisionOrigin::Endpoints,
        DomainOrigin::Rollback => ConfigRevisionOrigin::Rollback,
        DomainOrigin::Restored => ConfigRevisionOrigin::Restored,
    }
}

/// A domain port request on the wire.
#[must_use]
pub fn port_request(request: DomainPort) -> PortRequest {
    match request {
        DomainPort::Fixed(port) => PortRequest::Fixed(port.get()),
        DomainPort::Auto => PortRequest::Auto,
    }
}

fn request_text(request: &stackly_domain::EndpointRequests) -> Vec<EndpointRequest> {
    request
        .iter()
        .map(|(name, request)| EndpointRequest {
            name: name.to_string(),
            port: port_request(request),
        })
        .collect()
}

/// A revision of the history on the wire. Its values are the stored text:
/// a secret field holds its reference, never a value.
#[must_use]
pub fn revision_summary(
    record: &ConfigRevisionRecord,
    active: DomainNumber,
    last_known_good: Option<DomainNumber>,
) -> ConfigRevisionSummary {
    let values =
        serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(record.values.as_str())
            .unwrap_or_default();
    ConfigRevisionSummary {
        number: number(record.number),
        created_at: Timestamp::from_jiff(record.created_at),
        origin: origin(record.origin),
        source_number: record.source_number.map(number),
        ready_at: record.ready_at.map(Timestamp::from_jiff),
        is_active: record.number == active,
        is_last_known_good: last_known_good == Some(record.number),
        values: SettingsDocument::new(values),
        endpoints: request_text(&record.endpoints),
    }
}

/// One settings field of the page: the create form field plus whether the
/// page locks it (managed, set once, or a secret).
#[must_use]
pub fn config_field(field: &SettingField) -> ConfigField {
    let locked = field.classes().iter().any(|class| {
        matches!(
            class,
            SettingClass::Managed | SettingClass::Secret | SettingClass::ImmutableAfterInit
        )
    }) || field.apply() == Some(ApplyClass::ImmutableAfterInit);
    ConfigField {
        field: form_field(field),
        locked,
    }
}

/// The endpoints of the instance as the page shows them.
#[must_use]
pub fn endpoints(instance: &Instance) -> Vec<ConfigEndpoint> {
    instance
        .endpoints()
        .iter()
        .map(|endpoint| match endpoint {
            Endpoint::Ip(ip) => ConfigEndpoint {
                name: ip.name().to_string(),
                transport: match ip.transport() {
                    Transport::Tcp => EndpointTransport::Tcp,
                    Transport::Udp => EndpointTransport::Udp,
                },
                host: Some(ip.address().to_string()),
                request: Some(port_request(ip.request())),
                bound_port: ip.bound().map(|port| port.get()),
                changeable: true,
            },
            Endpoint::Unix(unix) => ConfigEndpoint {
                name: unix.name().to_string(),
                transport: EndpointTransport::Unix,
                host: None,
                request: None,
                bound_port: None,
                changeable: false,
            },
        })
        .collect()
}

/// `text` cut to [`FILE_BYTES_MAX`] bytes at a line end.
fn cut(text: &str) -> String {
    if text.len() <= FILE_BYTES_MAX {
        return text.to_owned();
    }
    let mut end = FILE_BYTES_MAX;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let kept = &text[..end];
    let kept = kept.rfind('\n').map_or(kept, |at| &kept[..=at]);
    format!("{kept}…\n")
}

/// A config file on the wire.
#[must_use]
pub fn effective_file(file: &CoreFile) -> EffectiveFile {
    EffectiveFile {
        path: file.path.clone(),
        content: cut(&file.content),
        changed_lines: file.changed_lines.clone(),
        preview: file.preview,
        port_note: file.port_note.clone(),
    }
}

/// A changed setting on the wire. A value is its JSON text, except that a
/// string is its text.
#[must_use]
pub fn setting_change(change: &CoreChange) -> SettingChange {
    let text = |value: &serde_json::Value| match value {
        serde_json::Value::String(text) => text.clone(),
        other => other.to_string(),
    };
    SettingChange {
        key: change.key.clone(),
        old: change.old.as_ref().map(text),
        new: change.new.as_ref().map(text),
    }
}

/// The ports that change on the wire.
#[must_use]
pub fn endpoint_changes(changes: &[(EndpointName, DomainPort, DomainPort)]) -> Vec<EndpointChange> {
    changes
        .iter()
        .map(|(name, old, new)| EndpointChange {
            name: name.to_string(),
            old: port_request(*old),
            new: port_request(*new),
        })
        .collect()
}

/// A request that cannot be applied, with the free port to try.
#[must_use]
pub fn endpoint_problem(issue: &EndpointIssue, code: &str) -> EndpointProblem {
    EndpointProblem {
        endpoint: issue.endpoint.clone(),
        code: code.to_owned(),
        suggested_port: issue.suggestion.map(|port| port.get()),
    }
}

/// The line diff of a drifted file: the copy Stackly wrote against what is
/// on disk. A line only in the copy is `removed`, a line only on disk is
/// `added`. Both texts are cut to [`DIFF_LINES_MAX`] lines first.
#[must_use]
pub fn diff_lines(copy: &str, disk: &str) -> Vec<DiffLine> {
    let old: Vec<&str> = copy.lines().take(DIFF_LINES_MAX).collect();
    let new: Vec<&str> = disk.lines().take(DIFF_LINES_MAX).collect();
    // lengths[i][j]: the longest common sequence of old[i..] and new[j..].
    let mut lengths = vec![vec![0_usize; new.len() + 1]; old.len() + 1];
    for i in (0..old.len()).rev() {
        for j in (0..new.len()).rev() {
            lengths[i][j] = if old[i] == new[j] {
                lengths[i + 1][j + 1] + 1
            } else {
                lengths[i + 1][j].max(lengths[i][j + 1])
            };
        }
    }
    let line = |kind, text: &str| DiffLine {
        kind,
        text: text.to_owned(),
    };
    let (mut i, mut j) = (0, 0);
    let mut diff = Vec::new();
    while i < old.len() && j < new.len() {
        if old[i] == new[j] {
            diff.push(line(DiffLineKind::Context, old[i]));
            i += 1;
            j += 1;
        } else if lengths[i + 1][j] >= lengths[i][j + 1] {
            diff.push(line(DiffLineKind::Removed, old[i]));
            i += 1;
        } else {
            diff.push(line(DiffLineKind::Added, new[j]));
            j += 1;
        }
    }
    diff.extend(
        old[i..]
            .iter()
            .map(|text| line(DiffLineKind::Removed, text)),
    );
    diff.extend(new[j..].iter().map(|text| line(DiffLineKind::Added, text)));
    diff
}

/// The drift of the active revision on the wire, `None` when clean.
#[must_use]
pub fn drift(report: &DriftReport, observed_at: jiff::Timestamp) -> Option<ConfigDrift> {
    if report.is_clean() {
        return None;
    }
    Some(ConfigDrift {
        observed_at: Timestamp::from_jiff(observed_at),
        files: report
            .files
            .iter()
            .map(|file| {
                let copy = String::from_utf8_lossy(&file.copy);
                let disk = file
                    .disk
                    .as_ref()
                    .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
                    .unwrap_or_default();
                DriftFile {
                    path: file.path.to_string(),
                    kind: match file.kind {
                        CoreDriftKind::Modified => DriftKind::Modified,
                        CoreDriftKind::Missing => DriftKind::Missing,
                    },
                    diff: diff_lines(&copy, &disk),
                }
            })
            .collect(),
    })
}

/// The view of `instance.config.get`.
#[must_use]
pub fn view(
    facts: &ConfigFacts,
    files: &[CoreFile],
    note: Option<String>,
    drift: Option<ConfigDrift>,
) -> InstanceConfigView {
    let active = facts.active.number;
    let good = facts.last_known_good.as_ref().map(|good| good.number);
    InstanceConfigView {
        revision: revision_summary(&facts.active, active, good),
        last_known_good: good.map(number),
        fields: facts
            .package
            .settings_schema()
            .fields()
            .map(config_field)
            .collect(),
        endpoints: endpoints(&facts.instance),
        history: facts
            .history
            .iter()
            .map(|record| revision_summary(record, active, good))
            .collect(),
        effective_files: files.iter().map(effective_file).collect(),
        effective_files_note: note,
        drift,
        reload_supported: facts.reload_supported,
    }
}

/// The text of every file by path, to mark the lines a proposal changes.
#[must_use]
pub fn contents(files: &[CoreFile]) -> BTreeMap<&str, &str> {
    files
        .iter()
        .map(|file| (file.path.as_str(), file.content.as_str()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(diff: &[DiffLine]) -> Vec<(DiffLineKind, &str)> {
        diff.iter()
            .map(|line| (line.kind, line.text.as_str()))
            .collect()
    }

    #[test]
    fn a_changed_line_is_removed_from_the_copy_and_added_from_the_disk() {
        let diff = diff_lines("a = 1\nb = 2\nc = 3\n", "a = 1\nb = 20\nc = 3\n");

        assert_eq!(
            kinds(&diff),
            [
                (DiffLineKind::Context, "a = 1"),
                (DiffLineKind::Removed, "b = 2"),
                (DiffLineKind::Added, "b = 20"),
                (DiffLineKind::Context, "c = 3"),
            ]
        );
    }

    #[test]
    fn a_missing_file_removes_every_line_of_its_copy() {
        let diff = diff_lines("a\nb\n", "");

        assert_eq!(
            kinds(&diff),
            [(DiffLineKind::Removed, "a"), (DiffLineKind::Removed, "b")]
        );
    }

    #[test]
    fn a_diff_is_cut_to_the_most_lines() {
        let copy = "x\n".repeat(DIFF_LINES_MAX + 50);

        assert_eq!(diff_lines(&copy, "").len(), DIFF_LINES_MAX);
    }

    #[test]
    fn a_long_file_is_cut_at_a_line_end_with_an_ellipsis() {
        let long = "line of text\n".repeat(FILE_BYTES_MAX / 10);
        let shown = cut(&long);

        assert!(shown.len() <= FILE_BYTES_MAX + 4);
        assert!(shown.ends_with("…\n"));
        assert!(shown.starts_with("line of text\n"));
        assert_eq!(cut("short\n"), "short\n");
    }

    #[test]
    fn a_string_change_shows_its_text_and_a_number_its_json() {
        let change = CoreChange {
            key: "greeting".to_owned(),
            old: Some(serde_json::json!("hello")),
            new: Some(serde_json::json!(4)),
        };

        let wire = setting_change(&change);

        assert_eq!(wire.old.as_deref(), Some("hello"));
        assert_eq!(wire.new.as_deref(), Some("4"));
    }

    #[test]
    fn the_impact_text_of_the_journal_reads_back() {
        for impact in [
            CoreImpact::None,
            CoreImpact::NextStart,
            CoreImpact::Reload,
            CoreImpact::Restart,
        ] {
            assert_eq!(impact_of_text(impact_text(impact)), super::impact(impact));
        }
        assert_eq!(impact_of_text("sideways"), ConfigImpact::Unrecognized);
    }
}
