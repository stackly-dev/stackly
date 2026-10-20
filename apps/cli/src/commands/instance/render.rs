//! The text of one instance: a `kv` block, the endpoint table, and the
//! failure output (spec 0014 AC-7, spec 0022 *CLI*). `get` writes it now
//! and `inspect` will write the same text, so label, order, and format
//! stay here.

use stackly_protocol::instance::{
    EndpointStatus, EndpointTransport, InstanceBlocker, InstanceHealth, InstanceLastFailure,
    InstanceLifecycle, InstanceProcess, InstanceReadiness, InstanceRestart, InstanceSummary,
};

use crate::output::escape::escape_display;
use crate::output::human::{Cell, HumanWriter, wire_name};
use crate::output::style::Tone;

/// The characters of the manifest digest that are shown.
const DIGEST_SHOWN: usize = 12;

/// Writes `instance` as the block, the endpoints, and the failure output.
pub fn render_instance(out: &mut HumanWriter, instance: &InstanceSummary) {
    let mut rows = vec![
        ("Name", Cell::text(&instance.name)),
        (
            "Display name",
            Cell::optional(instance.display_name.as_deref()),
        ),
        ("ID", Cell::fixed(instance.id.to_string())),
        (
            "Provider",
            Cell::text(&format!(
                "{} revision {}",
                instance.provider.name, instance.provider.revision
            )),
        ),
        (
            "Manifest",
            Cell::text(&manifest(&instance.provider.manifest_sha256)),
        ),
        ("Software", Cell::text(&instance.software)),
        ("Runtime", Cell::fixed(instance.runtime_id.to_string())),
        ("Version", Cell::text(&instance.runtime_version)),
        ("Ownership", Cell::text(&wire_name(&instance.ownership))),
        (
            "Initialized",
            Cell::fixed(if instance.is_initialized() {
                "yes"
            } else {
                "no"
            }),
        ),
        (
            "Lifecycle",
            Cell::toned(
                &wire_name(&instance.lifecycle),
                lifecycle_tone(instance.lifecycle),
            ),
        ),
        ("Desired", Cell::text(&wire_name(&instance.desired))),
        (
            "Readiness",
            Cell::text(&readiness(out, &instance.readiness)),
        ),
        ("Health", Cell::text(&health(out, &instance.health))),
        (
            "Process",
            Cell::text(
                &instance
                    .process
                    .as_ref()
                    .map_or_else(String::new, |p| process(out, p)),
            ),
        ),
        ("Restarts", Cell::text(&restarts(out, &instance.restart))),
        (
            "Last failure",
            Cell::text(
                &instance
                    .last_failure
                    .as_ref()
                    .map_or_else(String::new, |f| last_failure(out, f)),
            ),
        ),
        (
            "Blocker",
            Cell::text(&instance.blocker.as_ref().map_or_else(String::new, blocker)),
        ),
        (
            "Logs",
            Cell::fixed(if instance.logs_unavailable {
                "unavailable"
            } else {
                "available"
            }),
        ),
    ];
    if !instance.leftover_processes.is_empty() {
        let pids: Vec<String> = instance
            .leftover_processes
            .iter()
            .map(u32::to_string)
            .collect();
        rows.push(("Leftover processes", Cell::fixed(pids.join(", "))));
    }
    rows.push(("Revision", Cell::fixed(instance.revision.get().to_string())));
    out.kv(&rows);
    if !instance.endpoints.is_empty() {
        out.line("");
        let rows: Vec<Vec<Cell>> = instance.endpoints.iter().map(endpoint).collect();
        out.table(
            &["endpoint", "transport", "address", "port mode", "bound"],
            &rows,
        );
    }
    if let Some(failure) = &instance.last_failure
        && !failure.excerpt.is_empty()
    {
        out.line("");
        out.line("Failure output:");
        for line in &failure.excerpt {
            out.line(&format!("  {}", escape_display(line)));
        }
    }
}

/// `sha256:` and the first 12 characters of the digest.
fn manifest(digest: &str) -> String {
    let shown: String = digest.chars().take(DIGEST_SHOWN).collect();
    format!("sha256:{shown}")
}

/// `<state> (<reason>): <detail>, checked <age>`.
fn readiness(out: &HumanWriter, readiness: &InstanceReadiness) -> String {
    observed(
        out,
        &wire_name(&readiness.state),
        &readiness.reason,
        readiness.detail.as_deref(),
        readiness.observed_at,
    )
}

/// The same line for health.
fn health(out: &HumanWriter, health: &InstanceHealth) -> String {
    observed(
        out,
        &wire_name(&health.state),
        &health.reason,
        health.detail.as_deref(),
        health.observed_at,
    )
}

fn observed(
    out: &HumanWriter,
    state: &str,
    reason: &str,
    detail: Option<&str>,
    at: stackly_protocol::Timestamp,
) -> String {
    let mut text = state.to_owned();
    if !reason.is_empty() {
        text.push_str(&format!(" ({reason})"));
    }
    if let Some(detail) = detail.filter(|detail| !detail.is_empty()) {
        text.push_str(&format!(": {detail}"));
    }
    text.push_str(&format!(", checked {}", out.age(at)));
    text
}

/// `pid <pid>, started <time> (<age>)` with `, adopted` and
/// `, not adoptable`.
fn process(out: &HumanWriter, process: &InstanceProcess) -> String {
    let mut text = format!(
        "pid {}, started {}",
        process.pid,
        out.time_and_age(process.started_at)
    );
    if process.adopted {
        text.push_str(", adopted");
    }
    if process.adoptable == Some(false) {
        text.push_str(", not adoptable");
    }
    text
}

/// `<attempts> of <budget> in <minutes>m`, then `, next attempt <time>` and
/// `, exhausted`.
fn restarts(out: &HumanWriter, restart: &InstanceRestart) -> String {
    let mut text = format!(
        "{} of {} in {}m",
        restart.attempts,
        restart.budget,
        restart.window_seconds.div_ceil(60)
    );
    if let Some(at) = restart.next_attempt_at {
        text.push_str(&format!(", next attempt {}", out.time(at)));
    }
    if restart.exhausted {
        text.push_str(", exhausted");
    }
    text
}

/// `<code> at <time>`, then `, exit code <n>` or `, signal <n>`.
fn last_failure(out: &HumanWriter, failure: &InstanceLastFailure) -> String {
    let mut text = failure.code.clone();
    if let Some(at) = failure.at {
        text.push_str(&format!(" at {}", out.time(at)));
    }
    if let Some(code) = failure.exit_code {
        text.push_str(&format!(", exit code {code}"));
    } else if let Some(signal) = failure.signal {
        text.push_str(&format!(", signal {signal}"));
    }
    text
}

/// What stops the instance, in words.
fn blocker(blocker: &InstanceBlocker) -> String {
    match blocker {
        InstanceBlocker::IdentityUnproven(content) => {
            with_pid("identity unproven", content.evidence.pid)
        }
        InstanceBlocker::DataInUse(content) => with_pid("data in use", content.evidence.pid),
        InstanceBlocker::NeedsSecret(content) => {
            let references: Vec<String> = content
                .references
                .iter()
                .map(|item| format!("{} ({})", item.reference, wire_name(&item.reason)))
                .collect();
            format!("needs secret: {}", references.join(", "))
        }
    }
}

fn with_pid(text: &str, pid: Option<u32>) -> String {
    pid.map_or_else(|| text.to_owned(), |pid| format!("{text}, pid {pid}"))
}

/// One row of the endpoint table.
fn endpoint(endpoint: &EndpointStatus) -> Vec<Cell> {
    vec![
        Cell::text(&endpoint.name),
        Cell::text(&wire_name(&endpoint.transport)),
        Cell::text(&address(endpoint)),
        Cell::text(&wire_name(&endpoint.port_mode)),
        Cell::fixed(if endpoint.bound { "yes" } else { "no" }),
    ]
}

/// `<host>:<port>` (IPv6 in brackets, `auto` for a port not known yet), the
/// socket path of a unix endpoint, or `socket (pending)`.
fn address(endpoint: &EndpointStatus) -> String {
    if endpoint.transport == EndpointTransport::Unix {
        return endpoint
            .socket_path
            .clone()
            .unwrap_or_else(|| "socket (pending)".to_owned());
    }
    let Some(host) = endpoint.host.as_deref() else {
        return String::new();
    };
    let port = endpoint
        .port
        .map_or_else(|| "auto".to_owned(), |port| port.to_string());
    if host.contains(':') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    }
}

/// The color of a lifecycle on a terminal.
const fn lifecycle_tone(lifecycle: InstanceLifecycle) -> Tone {
    match lifecycle {
        InstanceLifecycle::Running => Tone::Positive,
        InstanceLifecycle::Failed => Tone::Critical,
        InstanceLifecycle::Unknown | InstanceLifecycle::Recovering => Tone::Caution,
        InstanceLifecycle::Initializing
        | InstanceLifecycle::Starting
        | InstanceLifecycle::Stopping => Tone::Informative,
        InstanceLifecycle::Uninitialized
        | InstanceLifecycle::Stopped
        | InstanceLifecycle::Retained => Tone::Neutral,
    }
}
