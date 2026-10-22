//! `stackly instance connection` (spec 0021, AC-6): the connection strings
//! of an instance, masked unless `--show-password` asks for the real
//! password.
//!
//! The daemon renders each string from the pinned package and sends it as
//! text and secret references (`instance.get_detail`). Only this command
//! joins the parts with a value, and only after `secret.reveal { purpose:
//! show }` for each distinct reference. Without `--show-password` it never
//! calls `secret.reveal`, so the Keychain is not read.

use std::collections::BTreeMap;

use clap::Args;
use serde::Serialize;
use stackly_protocol::instance::connection::{ConnectionPartKind, compose};
use stackly_protocol::instance::{
    ConnectionAvailability, ConnectionString, ConnectionUnavailableReason, InstanceDetail,
};
use stackly_protocol::secret::{RevealPurpose, RevealSecretParams};
use stackly_protocol::{Call, Reply};
use uuid::Uuid;

use super::read_detail;
use super::read_instance;
use super::resolve::TargetArg;
use crate::commands::RunCommand;
use crate::context::Context;
use crate::failure::CliFailure;
use crate::output::human::{Cell, HumanWriter, Render, wire_name};
use crate::output::{CommandOutput, OutputMode};
use crate::session::{CallSpec, RequestTimeout};

/// The note on stderr for a connection whose port can change.
const PORT_MAY_CHANGE: &str = "note: the port can change the next time the instance starts";

/// `stackly instance connection`.
#[derive(Debug, Args)]
pub struct ConnectionArgs {
    /// The instance.
    #[command(flatten)]
    pub instance: TargetArg,
    /// The connection to show, by name; the first one when absent
    #[arg(long, value_name = "NAME", conflicts_with = "all")]
    pub format: Option<String>,
    /// Show every connection of the instance, one per line
    #[arg(long, conflicts_with_all = ["format", "show_password"])]
    pub all: bool,
    /// Write the real password in the string instead of four stars
    #[arg(long)]
    pub show_password: bool,
}

/// The instance a connection belongs to.
#[derive(Debug, Clone, Serialize)]
pub struct InstanceRef {
    /// Its ID.
    pub id: Uuid,
    /// Its name.
    pub name: String,
}

/// The `result` of `connection`: one string, or every string with `--all`.
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum ConnectionOutput {
    /// The first string or the one `--format` named.
    One {
        /// The instance.
        instance: InstanceRef,
        /// The string as the daemon described it, never with a value.
        connection: Box<ConnectionString>,
        /// The whole string with the password; set only with
        /// `--show-password`.
        value: Option<String>,
        /// What text mode writes: the masked string or `value`.
        #[serde(skip)]
        text: String,
    },
    /// Every string, in manifest order.
    All {
        /// The instance.
        instance: InstanceRef,
        /// The strings.
        connections: Vec<ConnectionString>,
    },
}

impl RunCommand for ConnectionArgs {
    type Output = ConnectionOutput;

    async fn run(self, cx: &mut Context) -> CommandOutput<ConnectionOutput> {
        match run(self, cx).await {
            Ok(output) => output,
            Err(failure) => CommandOutput::failed(failure),
        }
    }
}

async fn run(
    args: ConnectionArgs,
    cx: &mut Context,
) -> Result<CommandOutput<ConnectionOutput>, CliFailure> {
    let (id, known) = args.instance.target.resolve_named(cx).await?;
    let detail = read_detail(cx, id).await?;
    if !detail.provider.package_readable {
        return Err(CliFailure::connection_unavailable(
            args.format.as_deref(),
            "package_unavailable",
            true,
        ));
    }
    if detail.connections.is_empty() {
        return Err(not_found(args.format.as_deref(), &detail));
    }
    let name = match known {
        Some(name) => name,
        None => read_instance(cx, id).await?.name,
    };
    let instance = InstanceRef { id, name };
    if args.all {
        return Ok(CommandOutput::ok(ConnectionOutput::All {
            instance,
            connections: detail.connections,
        }));
    }
    let chosen = match args.format.as_deref() {
        Some(wanted) => detail
            .connections
            .iter()
            .find(|connection| connection.name == wanted)
            .ok_or_else(|| not_found(Some(wanted), &detail))?,
        None => &detail.connections[0],
    };
    if let Some(reason) = unavailable_reason(chosen) {
        return Err(CliFailure::connection_unavailable(
            Some(&chosen.name),
            &wire_name(&reason),
            false,
        ));
    }
    let masked = chosen.masked.clone().unwrap_or_default();
    let (text, value) = if args.show_password {
        let full = reveal(cx, chosen).await?;
        (full.clone(), Some(full))
    } else {
        (masked, None)
    };
    if chosen.port_may_change && cx.output.mode == OutputMode::Human && !cx.output.quiet {
        cx.output.stderr.line(PORT_MAY_CHANGE);
    }
    Ok(CommandOutput::ok(ConnectionOutput::One {
        instance,
        connection: Box::new(chosen.clone()),
        value,
        text,
    }))
}

/// `cli.connection_not_found` for `format`, with the names `detail` has.
fn not_found(format: Option<&str>, detail: &InstanceDetail) -> CliFailure {
    let available: Vec<&str> = detail
        .connections
        .iter()
        .map(|connection| connection.name.as_str())
        .collect();
    CliFailure::connection_not_found(format, &available)
}

/// Why `connection` cannot be given as a string, or `None` when it can. A
/// state or a part this build does not know counts as unavailable.
fn unavailable_reason(connection: &ConnectionString) -> Option<ConnectionUnavailableReason> {
    match connection.state {
        ConnectionAvailability::Available => connection
            .parts
            .iter()
            .any(|part| part.kind == ConnectionPartKind::Unrecognized)
            .then_some(ConnectionUnavailableReason::Unrecognized),
        ConnectionAvailability::Unavailable => Some(
            connection
                .reason
                .unwrap_or(ConnectionUnavailableReason::Unrecognized),
        ),
        ConnectionAvailability::Unrecognized => Some(ConnectionUnavailableReason::Unrecognized),
    }
}

/// `secret.reveal { purpose: show }` once per distinct reference of
/// `connection`, then the whole string.
async fn reveal(cx: &mut Context, connection: &ConnectionString) -> Result<String, CliFailure> {
    let mut values = BTreeMap::new();
    for part in &connection.parts {
        let Some(secret) = &part.secret else { continue };
        if values.contains_key(&secret.reference) {
            continue;
        }
        let params = RevealSecretParams {
            reference: secret.reference.clone(),
            purpose: RevealPurpose::Show,
        };
        let spec = CallSpec {
            timeout: if cx.prompt_allowed() {
                RequestTimeout::Interactive
            } else {
                RequestTimeout::Control
            },
            idempotency_key: None,
        };
        match cx.call(Call::SecretReveal(params), spec).await? {
            Reply::SecretReveal(reply) => {
                values.insert(secret.reference.clone(), reply.value.expose().to_owned());
            }
            _ => return Err(CliFailure::unexpected_reply(None)),
        }
    }
    compose(&connection.parts, &values).map_err(|_| CliFailure::unexpected_reply(None))
}

impl Render for ConnectionOutput {
    fn render(&self, out: &mut HumanWriter) {
        match self {
            Self::One { text, .. } => out.line(text),
            Self::All { connections, .. } => {
                let rows: Vec<(&str, Cell)> = connections
                    .iter()
                    .map(|connection| {
                        let value = match unavailable_reason(connection) {
                            None => connection.masked.clone().unwrap_or_default(),
                            Some(reason) => format!("unavailable ({})", wire_name(&reason)),
                        };
                        (connection.name.as_str(), Cell::text(&value))
                    })
                    .collect();
                out.kv(&rows);
            }
        }
    }
}
