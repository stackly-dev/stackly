//! `stackly audit list` (spec 0035, AC-16): the local audit trail the
//! daemon keeps, newest first. It only reads `audit.list`; `--json` is the
//! way to export the trail (spec 0035, *Batas dengan spec lain*).
//!
//! A row never holds a secret value, because the daemon never stores one.

use clap::{Args, Subcommand};
use stackly_protocol::audit::{
    AUDIT_LIST_LIMIT_DEFAULT, AUDIT_LIST_LIMIT_MAX, AuditActorKind, AuditEventInfo, AuditPage,
    AuditResultKind, ListAuditParams,
};
use stackly_protocol::{Call, Reply};
use uuid::Uuid;

use super::{RunCommand, execute};
use crate::args;
use crate::commands::instance::resolve::InstanceTarget;
use crate::context::Context;
use crate::failure::CliFailure;
use crate::output::CommandOutput;
use crate::output::human::{Cell, HumanWriter, Render, wire_name};
use crate::output::style::Tone;
use crate::session::CallSpec;

/// The fewest columns DETAILS keeps when the terminal is narrower than the
/// other columns, so the error code still shows.
const DETAILS_MIN_COLUMNS: usize = 24;

/// The verbs of `stackly audit`.
#[derive(Debug, Subcommand)]
pub enum AuditCommand {
    /// List the audit trail, newest first: the latest 50 entries by default
    #[command(
        after_help = "Examples:\n  stackly audit list\n  stackly audit list --instance pg17-dev --limit 20\n  stackly audit list --operation 0192f3a0-7c1e-7b2a-9d4e-5f6a7b8c9d0e --json"
    )]
    List(ListArgs),
}

impl AuditCommand {
    /// Runs the verb and returns the exit code.
    pub async fn run(self, cx: &mut Context) -> u8 {
        match self {
            Self::List(args) => execute(args, cx).await,
        }
    }
}

/// `stackly audit list`.
#[derive(Debug, Args)]
pub struct ListArgs {
    /// Only entries about this instance: its name, or its whole ID, also of an instance that is gone
    #[arg(long, value_name = "INSTANCE")]
    pub instance: Option<InstanceTarget>,
    /// Only entries of this operation, its whole ID
    #[arg(long, value_name = "ID", value_parser = args::operation_id)]
    pub operation: Option<Uuid>,
    /// How many entries, from 1 to 200 (default 50)
    #[arg(long, value_name = "N", value_parser = clap::value_parser!(u32).range(1..=i64::from(AUDIT_LIST_LIMIT_MAX)))]
    pub limit: Option<u32>,
    /// Go on after the page that printed this next value
    #[arg(long, value_name = "CURSOR", value_parser = args::cursor)]
    pub before: Option<String>,
}

impl RunCommand for ListArgs {
    type Output = AuditPage;

    async fn run(self, cx: &mut Context) -> CommandOutput<AuditPage> {
        // A whole UUID goes as it is, so the trail of a removed instance
        // can still be read; only a name is looked up.
        let instance_id = match &self.instance {
            Some(target) => match target.resolve(cx).await {
                Ok(id) => Some(id),
                Err(failure) => return CommandOutput::failed(failure),
            },
            None => None,
        };
        let params = ListAuditParams {
            instance_id,
            operation_id: self.operation,
            limit: Some(self.limit.unwrap_or(AUDIT_LIST_LIMIT_DEFAULT)),
            before: self.before.clone(),
        };
        let page = match cx.call(Call::AuditList(params), CallSpec::control()).await {
            Ok(Reply::AuditList(page)) => page,
            Ok(_) => return CommandOutput::failed(CliFailure::unexpected_reply(None)),
            Err(failure) => return CommandOutput::failed(failure),
        };
        let footer = page.next.as_ref().map(|next| self.footer(next));
        let output = CommandOutput::ok(page);
        match footer {
            Some(footer) => output.with_hint(footer),
            None => output,
        }
    }
}

impl ListArgs {
    /// The hint that repeats this command's flags, as they were given, for
    /// the next page.
    fn footer(&self, next: &str) -> String {
        let mut text = "more audit entries: stackly audit list".to_owned();
        if let Some(instance) = &self.instance {
            text.push_str(&format!(" --instance {instance}"));
        }
        if let Some(operation) = self.operation {
            text.push_str(&format!(" --operation {operation}"));
        }
        if let Some(limit) = self.limit {
            text.push_str(&format!(" --limit {limit}"));
        }
        text.push_str(&format!(" --before {next}"));
        text
    }
}

impl Render for AuditPage {
    fn render(&self, out: &mut HumanWriter) {
        if self.items.is_empty() {
            out.line("No audit entries.");
            return;
        }
        let rows: Vec<Vec<Cell>> = self
            .items
            .iter()
            .map(|item| {
                vec![
                    Cell::fixed(out.time(item.occurred_at)),
                    Cell::text(&by_text(item)),
                    Cell::text(&action_text(item)),
                    Cell::toned(&wire_name(&item.result), result_tone(item.result)),
                    Cell::fixed(
                        item.operation_id
                            .map(|id| id.to_string())
                            .unwrap_or_default(),
                    ),
                    Cell::text(&details_text(item)),
                ]
            })
            .collect();
        out.fitted_table(
            &["time", "by", "action", "result", "operation", "details"],
            &rows,
            DETAILS_MIN_COLUMNS,
        );
    }
}

/// Who acted: `user (cli)`, `user (gui)`, or `daemon`.
fn by_text(item: &AuditEventInfo) -> String {
    let actor = match item.actor {
        AuditActorKind::User => "user".to_owned(),
        AuditActorKind::Daemon => "daemon".to_owned(),
        AuditActorKind::Unrecognized => "unknown".to_owned(),
    };
    match &item.client {
        Some(client) => format!("{actor} ({})", wire_name(client)),
        None => actor,
    }
}

/// The action, with the kind of operation when it has one:
/// `operation.finished (start_instance)`.
fn action_text(item: &AuditEventInfo) -> String {
    match &item.operation_kind {
        Some(kind) => format!("{} ({})", item.action, wire_name(kind)),
        None => item.action.clone(),
    }
}

/// The tone of a result, like the tone of the operation state it names.
const fn result_tone(result: AuditResultKind) -> Tone {
    match result {
        AuditResultKind::Succeeded | AuditResultKind::Cancelled => Tone::Neutral,
        AuditResultKind::Failed | AuditResultKind::NeedsRecovery => Tone::Critical,
        AuditResultKind::Unrecognized => Tone::Caution,
    }
}

/// DETAILS (spec 0035, *CLI*): `errorCode` first, then `configRevision`,
/// then every other detail in the daemon's key order, as `key=value` pairs
/// apart by one space; a value that is empty or holds a space, a quote, or
/// `=` is quoted.
fn details_text(item: &AuditEventInfo) -> String {
    let mut pairs = Vec::new();
    if let Some(code) = item.details.iter().find(|detail| detail.key == "errorCode") {
        pairs.push(pair(&code.key, &code.value));
    }
    if let Some(revision) = item.config_revision {
        pairs.push(format!("configRevision={}", revision.get()));
    }
    pairs.extend(
        item.details
            .iter()
            .filter(|detail| detail.key != "errorCode")
            .map(|detail| pair(&detail.key, &detail.value)),
    );
    pairs.join(" ")
}

/// `key=value`, with the value quoted when it would not read as one word.
fn pair(key: &str, value: &str) -> String {
    let plain = !value.is_empty()
        && !value
            .chars()
            .any(|c| c.is_whitespace() || matches!(c, '"' | '=' | '\\'));
    if plain {
        format!("{key}={value}")
    } else {
        let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
        format!("{key}=\"{escaped}\"")
    }
}

#[cfg(test)]
mod tests {
    use stackly_protocol::Count;
    use stackly_protocol::audit::AuditDetail;
    use stackly_protocol::handshake::ClientKind;
    use stackly_protocol::operation::OperationKind;

    use super::*;

    const ID: &str = "0192f3a0-7c1e-7b2a-9d4e-5f6a7b8c9d0e";

    fn row() -> AuditEventInfo {
        AuditEventInfo {
            id: Uuid::parse_str(ID).unwrap(),
            occurred_at: "2026-11-10T08:00:00Z".parse().unwrap(),
            actor: AuditActorKind::User,
            client: Some(ClientKind::Cli),
            action: "operation.finished".to_owned(),
            operation_kind: Some(OperationKind::StartInstance),
            operation_id: Some(Uuid::parse_str(ID).unwrap()),
            resource: None,
            instance_id: None,
            config_revision: Some(Count::new(3).unwrap()),
            result: AuditResultKind::Failed,
            details: [
                ("errorCode", "supervisor.endpoint_in_use"),
                ("message", "port 5432 is \"in\" use"),
                ("ports", "main 5432"),
                ("stage", "endpoints"),
                ("state", "failed"),
            ]
            .into_iter()
            .map(|(key, value)| AuditDetail {
                key: key.to_owned(),
                value: value.to_owned(),
            })
            .collect(),
        }
    }

    #[test]
    fn details_start_with_the_error_code_and_the_revision_then_follow_the_key_order() {
        assert_eq!(
            details_text(&row()),
            "errorCode=supervisor.endpoint_in_use configRevision=3 message=\"port 5432 is \\\"in\\\" use\" ports=\"main 5432\" stage=endpoints state=failed"
        );
        let bare = AuditEventInfo {
            config_revision: None,
            details: Vec::new(),
            ..row()
        };
        assert_eq!(details_text(&bare), "");
    }

    #[test]
    fn a_value_that_would_not_read_as_one_word_is_quoted() {
        assert_eq!(pair("k", "v"), "k=v");
        assert_eq!(pair("k", ""), "k=\"\"");
        assert_eq!(pair("k", "a=b"), "k=\"a=b\"");
        assert_eq!(pair("k", "a\\b"), "k=\"a\\\\b\"");
        assert_eq!(pair("k", "main 5432 → 5433"), "k=\"main 5432 → 5433\"");
        assert_eq!(pair("k", "5432→5433"), "k=5432→5433");
    }

    #[test]
    fn by_names_the_actor_and_the_client_it_came_through() {
        assert_eq!(by_text(&row()), "user (cli)");
        let daemon = AuditEventInfo {
            actor: AuditActorKind::Daemon,
            client: None,
            ..row()
        };
        assert_eq!(by_text(&daemon), "daemon");
        let secret = AuditEventInfo {
            action: "secret.reveal".to_owned(),
            operation_kind: None,
            ..row()
        };
        assert_eq!(action_text(&row()), "operation.finished (start_instance)");
        assert_eq!(action_text(&secret), "secret.reveal");
    }

    #[test]
    fn the_footer_repeats_the_flags_given_and_only_those() {
        let args = ListArgs {
            instance: Some(InstanceTarget::Name("pg17-dev".to_owned())),
            operation: Some(Uuid::parse_str(ID).unwrap()),
            limit: Some(20),
            before: Some("old".to_owned()),
        };
        assert_eq!(
            args.footer("next"),
            format!(
                "more audit entries: stackly audit list --instance pg17-dev --operation {ID} --limit 20 --before next"
            )
        );
        let bare = ListArgs {
            instance: None,
            operation: None,
            limit: None,
            before: None,
        };
        assert_eq!(
            bare.footer("n"),
            "more audit entries: stackly audit list --before n"
        );
    }
}
