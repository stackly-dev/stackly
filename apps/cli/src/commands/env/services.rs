//! Text about the services of an environment that `up` and `status` share
//! (spec 0046, *CLI*, AC-17, AC-18): the endpoints with the ports that are
//! really bound, the service tables, one sentence per reason a service is
//! not ready, and the progress line for each phase change of an Up. Every
//! value comes from the daemon and is escaped before it is written.

use std::collections::HashMap;

use stackly_protocol::env::{
    DependencyFailure, EnvPlan, EnvUpReport, EnvironmentDetail, EnvironmentService,
    EnvironmentStatus, NotReadyReason, ServiceNotReady, ServiceUpState, UpPhase,
};
use stackly_protocol::instance::EndpointStatus;
use stackly_protocol::secret::NeedsSecretItem;

use crate::commands::provider::local::esc;
use crate::output::human::{Cell, HumanWriter, wire_name};
use crate::output::style::Tone;

/// The `STATUS` cell of a ready service.
pub const READY: &str = "ready";

/// `<host>:<port>` of an endpoint that is bound, its socket path for a
/// `unix` one; `None` while it is not bound.
fn address(endpoint: &EndpointStatus) -> Option<String> {
    if !endpoint.bound {
        return None;
    }
    match (&endpoint.host, endpoint.port, &endpoint.socket_path) {
        (Some(host), Some(port), _) => Some(format!("{host}:{port}")),
        (_, _, Some(path)) => Some(path.clone()),
        (None, Some(port), None) => Some(format!("127.0.0.1:{port}")),
        _ => None,
    }
}

/// The `ENDPOINTS` cell: `<name> <host>:<port>` per endpoint, comma
/// separated, `<name> -` for one that is not bound, `-` without an
/// instance.
#[must_use]
pub fn endpoints_text(service: &EnvironmentService) -> String {
    let Some(instance) = &service.instance else {
        return "-".to_owned();
    };
    if instance.endpoints.is_empty() {
        return "-".to_owned();
    }
    instance
        .endpoints
        .iter()
        .map(|endpoint| {
            format!(
                "{} {}",
                endpoint.name,
                address(endpoint).unwrap_or_else(|| "-".to_owned())
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// The bound addresses of a service, comma separated, for the `ready`
/// progress line.
fn bound_addresses(service: &EnvironmentService) -> Vec<String> {
    service
        .instance
        .iter()
        .flat_map(|instance| instance.endpoints.iter().filter_map(address))
        .collect()
}

/// The `STATUS` cell: `ready`, or the reason it is not.
#[must_use]
pub fn status_text(service: &EnvironmentService) -> String {
    service.not_ready.as_ref().map_or_else(
        || READY.to_owned(),
        |not_ready| wire_name(&not_ready.reason),
    )
}

/// The color of a `STATUS` cell.
fn status_tone(service: &EnvironmentService) -> Tone {
    match service.not_ready.as_ref().map(|not_ready| not_ready.reason) {
        None => Tone::Positive,
        Some(NotReadyReason::BringingUp | NotReadyReason::Starting) => Tone::Neutral,
        Some(NotReadyReason::Stopped | NotReadyReason::NotCreated) => Tone::Neutral,
        Some(_) => Tone::Caution,
    }
}

/// The table of `env up --wait`: `SERVICE INSTANCE STATUS ENDPOINTS`.
pub fn up_table(out: &mut HumanWriter, detail: &EnvironmentDetail) {
    let rows: Vec<Vec<Cell>> = detail
        .services
        .iter()
        .map(|service| {
            vec![
                Cell::text(&service.alias),
                Cell::optional(service.instance.as_ref().map(|i| i.name.as_str())),
                Cell::toned(&status_text(service), status_tone(service)),
                Cell::text(&endpoints_text(service)),
            ]
        })
        .collect();
    out.table(&["service", "instance", "status", "endpoints"], &rows);
}

/// The table of `env status`: `SERVICE INSTANCE VERSION OWNERSHIP STATUS
/// ENDPOINTS` (AC-18). The version is the instance's runtime, else the
/// manifest selector.
pub fn status_table(out: &mut HumanWriter, detail: &EnvironmentDetail) {
    let rows: Vec<Vec<Cell>> = detail
        .services
        .iter()
        .map(|service| {
            let version = service
                .instance
                .as_ref()
                .map(|instance| instance.runtime_version.as_str())
                .or(service.selector.as_deref());
            vec![
                Cell::text(&service.alias),
                Cell::optional(service.instance.as_ref().map(|i| i.name.as_str())),
                Cell::optional(version),
                Cell::text(&wire_name(&service.ownership)),
                Cell::toned(&status_text(service), status_tone(service)),
                Cell::text(&endpoints_text(service)),
            ]
        })
        .collect();
    out.table(
        &[
            "service",
            "instance",
            "version",
            "ownership",
            "status",
            "endpoints",
        ],
        &rows,
    );
}

/// `<ready> of <total> ready.`
#[must_use]
pub fn ready_line(detail: &EnvironmentDetail) -> String {
    format!(
        "{} of {} ready.",
        u64::from(detail.summary.ready_count),
        u64::from(detail.summary.service_count)
    )
}

/// An environment status for people: `Ready`, `Partially ready`, `Down`.
#[must_use]
pub fn status_title(status: EnvironmentStatus) -> String {
    let text = wire_name(&status).replace('_', " ");
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

/// `<reference> (<reason>)`, comma separated.
fn references_text(references: &[NeedsSecretItem]) -> String {
    references
        .iter()
        .map(|item| format!("{} ({})", esc(&item.reference), wire_name(&item.reason)))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Why a dependency kept a service from starting, as words.
fn dependency_words(reason: Option<DependencyFailure>) -> String {
    match reason {
        Some(DependencyFailure::Failed) | None => "failed".to_owned(),
        Some(DependencyFailure::Timeout) => "was not ready in time".to_owned(),
        Some(DependencyFailure::NotReady) => "was not ready".to_owned(),
        Some(other) => wire_name(&other),
    }
}

/// One sentence for why a service is not ready, with the reference to set,
/// the failed stage, or the operation to inspect (spec 0046, *Alasan belum
/// ready*).
#[must_use]
pub fn not_ready_text(not_ready: &ServiceNotReady) -> String {
    let mut text = match not_ready.reason {
        NotReadyReason::BringingUp => {
            let phase = not_ready
                .phase
                .map_or_else(|| "pending".to_owned(), |phase| wire_name(&phase));
            if not_ready.waiting_on.is_empty() {
                format!("bringing up ({phase})")
            } else {
                format!(
                    "bringing up ({phase}), waiting for {}",
                    esc(&not_ready.waiting_on.join(", "))
                )
            }
        }
        NotReadyReason::NeedsSecret => {
            let mut text = format!("needs secret {}", references_text(&not_ready.references));
            for item in &not_ready.references {
                text.push_str(&format!(
                    "; set it with stackly secret set {}",
                    esc(&item.reference)
                ));
            }
            text
        }
        NotReadyReason::DependencyNotReady => format!(
            "not started: dependency {} {}",
            esc(not_ready.dependency.as_deref().unwrap_or("-")),
            dependency_words(not_ready.dependency_reason)
        ),
        NotReadyReason::UpFailed => {
            let stage = not_ready
                .stage
                .map_or_else(|| "up".to_owned(), |stage| wire_name(&stage));
            let code = not_ready.code.as_deref().map(esc).unwrap_or_default();
            if code.is_empty() {
                format!("the last Up failed at {stage}")
            } else {
                format!("the last Up failed at {stage}: {code}")
            }
        }
        other => wire_name(&other),
    };
    if let Some(detail) = &not_ready.detail {
        text.push_str(&format!(" ({})", esc(detail)));
    }
    if not_ready.reason == NotReadyReason::UpFailed
        && let Some(operation) = not_ready.operation_id
    {
        text.push_str(&format!(
            "; inspect with stackly operation inspect {operation}"
        ));
    }
    text
}

/// `not ready:` and one `  <service>: <reason>` line per service that is
/// not ready; nothing when every service is.
pub fn not_ready_lines(out: &mut HumanWriter, detail: &EnvironmentDetail) {
    let lines: Vec<String> = detail
        .services
        .iter()
        .filter_map(|service| {
            service.not_ready.as_ref().map(|not_ready| {
                format!("  {}: {}", esc(&service.alias), not_ready_text(not_ready))
            })
        })
        .collect();
    if lines.is_empty() {
        return;
    }
    out.line("not ready:");
    for line in lines {
        out.line(&line);
    }
}

/// What the plan says about one service, for its progress lines.
#[derive(Debug, Clone, Default)]
struct Planned {
    instance: Option<String>,
    provider: String,
    version: Option<String>,
}

/// The progress lines of an Up (spec 0046 AC-17): one line each time the
/// phase of a service in the report of this Up changes.
#[derive(Debug, Default)]
pub struct PhaseLines {
    planned: HashMap<String, Planned>,
    phases: HashMap<String, UpPhase>,
}

impl PhaseLines {
    /// Lines for the services of `plan`.
    #[must_use]
    pub fn new(plan: &EnvPlan) -> Self {
        let planned = plan
            .items
            .iter()
            .map(|item| {
                (
                    item.alias.clone(),
                    Planned {
                        instance: Some(item.instance.name.clone()),
                        provider: item.provider.clone(),
                        version: item.runtime.as_ref().map(|runtime| runtime.version.clone()),
                    },
                )
            })
            .collect();
        Self {
            planned,
            phases: HashMap::new(),
        }
    }

    /// The lines for every phase that changed since the last call, when
    /// the report of `detail` is the one of `operation`.
    pub fn changes(&mut self, detail: &EnvironmentDetail, operation: uuid::Uuid) -> Vec<String> {
        let Some(report) = detail
            .last_up
            .as_ref()
            .filter(|report| report.operation_id == operation)
        else {
            return Vec::new();
        };
        self.changes_in(detail, report)
    }

    fn changes_in(&mut self, detail: &EnvironmentDetail, report: &EnvUpReport) -> Vec<String> {
        let mut lines = Vec::new();
        for state in &report.services {
            if self.phases.get(&state.alias) == Some(&state.phase) {
                continue;
            }
            self.phases.insert(state.alias.clone(), state.phase);
            let service = detail
                .services
                .iter()
                .find(|service| service.alias == state.alias);
            if let Some(line) = self.line(state, service) {
                lines.push(line);
            }
        }
        lines
    }

    /// The line of the phase `state` is in now, `None` for `pending`.
    fn line(&self, state: &ServiceUpState, service: Option<&EnvironmentService>) -> Option<String> {
        let planned = self.planned.get(&state.alias).cloned().unwrap_or_default();
        let alias = esc(&state.alias);
        let instance = service
            .and_then(|service| service.instance.as_ref())
            .map(|instance| instance.name.clone())
            .or(planned.instance)
            .map_or_else(|| alias.clone(), |name| esc(&name));
        let line = match state.phase {
            UpPhase::Pending | UpPhase::Unrecognized => return None,
            UpPhase::Installing => format!(
                "install {} {}",
                esc(&planned.provider),
                esc(planned.version.as_deref().unwrap_or("-"))
            ),
            UpPhase::Creating => format!("create {instance}"),
            UpPhase::Starting => format!("start {instance}"),
            UpPhase::Waiting => format!(
                "wait {alias}: waiting for {}",
                esc(&state.waiting_on.join(", "))
            ),
            UpPhase::Ready => {
                let addresses = service.map(bound_addresses).unwrap_or_default();
                if addresses.is_empty() {
                    format!("ready {instance}")
                } else {
                    format!("ready {instance} {}", esc(&addresses.join(", ")))
                }
            }
            UpPhase::Skipped => format!(
                "skip {alias}: waiting for {}",
                esc(state.dependency.as_deref().unwrap_or("-"))
            ),
            UpPhase::Failed => {
                let stage = state
                    .stage
                    .map_or_else(|| "up".to_owned(), |stage| wire_name(&stage));
                let code = state.code.as_deref().map(esc).unwrap_or_default();
                format!("failed {alias}: {stage} {code}")
                    .trim_end()
                    .to_owned()
            }
            UpPhase::NeedsSecret => format!(
                "needs secret {alias}: {}",
                references_text(&state.references)
            ),
            UpPhase::Cancelled => format!("cancelled {alias}"),
        };
        Some(line)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn detail(name: &str) -> EnvironmentDetail {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../protocols/fixtures/env")
            .join(name);
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn a_bound_port_is_shown_and_an_unbound_one_is_a_dash() {
        let mut detail = detail("detail-up-incomplete.json");
        assert_eq!(endpoints_text(&detail.services[0]), "http 127.0.0.1:49152");
        assert_eq!(endpoints_text(&detail.services[1]), "-");
        let instance = detail.services[0].instance.as_mut().unwrap();
        instance.endpoints[0].bound = false;
        assert_eq!(endpoints_text(&detail.services[0]), "http -");
    }

    #[test]
    fn each_reason_names_what_to_do_next() {
        let detail = detail("detail-up-incomplete.json");
        let cache = detail.services[1].not_ready.as_ref().unwrap();
        assert_eq!(
            not_ready_text(cache),
            "needs secret local:graph/cache-token (missing); set it with stackly secret set local:graph/cache-token"
        );
        let api = detail.services[2].not_ready.as_ref().unwrap();
        assert_eq!(not_ready_text(api), "not started: dependency cache failed");
        let mut failed = api.clone();
        failed.reason = NotReadyReason::UpFailed;
        failed.stage = Some(stackly_protocol::env::UpStage::Start);
        failed.code = Some("supervisor.spawn_failed".to_owned());
        failed.operation_id = Some(uuid::Uuid::from_u128(7));
        assert_eq!(
            not_ready_text(&failed),
            "the last Up failed at start: supervisor.spawn_failed; inspect with stackly operation inspect 00000000-0000-0000-0000-000000000007"
        );
    }

    #[test]
    fn a_phase_change_gives_one_line_and_the_same_phase_none() {
        let running = detail("detail-bringing-up.json");
        let operation = running.last_up.as_ref().unwrap().operation_id;
        let mut lines = PhaseLines::default();
        assert_eq!(
            lines.changes(&running, operation),
            [
                "ready graph-db 127.0.0.1:49152",
                "wait api: waiting for cache",
                "start cache"
            ]
        );
        assert!(lines.changes(&running, operation).is_empty());
        let ended = detail("detail-up-incomplete.json");
        let lines_after: Vec<String> = lines.changes(&ended, operation);
        assert_eq!(
            lines_after,
            [
                "needs secret cache: local:graph/cache-token (missing)",
                "skip api: waiting for cache"
            ]
        );
        assert!(lines.changes(&ended, uuid::Uuid::from_u128(1)).is_empty());
    }

    #[test]
    fn a_status_reads_as_words() {
        assert_eq!(
            status_title(EnvironmentStatus::PartiallyReady),
            "Partially ready"
        );
        assert_eq!(status_title(EnvironmentStatus::Ready), "Ready");
    }
}
