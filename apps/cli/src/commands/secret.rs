//! `stackly secret list`, `status`, `set`, `reveal`, and `delete` (spec 0011,
//! *Input aman CLI* and *Permukaan API*, AC-16, AC-17). They only talk to
//! the daemon, which owns the Keychain. A value enters through
//! [`secret_input`](crate::secret_input) and leaves only through `reveal`:
//! no other output holds one.

use clap::{Args, Subcommand};
use stackly_protocol::codes::secret as secret_codes;
use stackly_protocol::secret::{
    DeleteSecretParams, DeleteSecretReply, ListSecretsParams, ListSecretsReply, RevealPurpose,
    RevealSecretParams, RevealSecretReply, SecretMode, SecretRefSummary, SecretStatus,
    SecretStatusParams, SecretStatusReply, SecretStoreStatus, SecretWarning, SetSecretParams,
    SetSecretReply,
};
use stackly_protocol::{Call, ErrorBody, Reply, Revision};

use super::{RunCommand, execute};
use crate::args;
use crate::confirm::{
    Approval, Confirm, ConfirmRequest, ConfirmRole, Confirmation, OnNoninteractive,
};
use crate::context::Context;
use crate::failure::CliFailure;
use crate::output::CommandOutput;
use crate::output::human::{Cell, HumanWriter, Render, wire_name};
use crate::output::style::Tone;
use crate::secret_input::{self, SourceArgs, Tty};
use crate::session::{CallSpec, RequestTimeout};

/// The approval flag of a replace that changes only the stored copy.
const CONFIRM_FLAG: &str = "--confirm-stored-only";

/// The verbs of `stackly secret`.
#[derive(Debug, Subcommand)]
pub enum SecretCommand {
    /// List the secret references and the state of the store; values are
    /// never shown
    #[command(after_help = "Examples:\n  stackly secret list\n  stackly secret list --json")]
    List(ListArgs),
    /// Check one secret reference against the store without showing its
    /// value
    #[command(
        after_help = "Examples:\n  stackly secret status local:user/api-token\n  stackly secret status local:user/api-token --json"
    )]
    Status(StatusArgs),
    /// Store a secret value: from a hidden prompt (asked twice), stdin, a
    /// file, or an environment variable, or let the daemon generate or copy
    /// one
    #[command(
        after_help = "Examples:\n  stackly secret set local:user/api-token\n  printf '%s' \"$TOKEN\" | stackly secret set local:user/api-token --stdin\n  stackly secret set local:user/api-token --from-file ./token --replace\n  stackly secret set local:user/api-token --from-env TOKEN --upsert --expected-revision 3\n  stackly secret set local:user/db-password --generate\n  stackly secret set local:user/copy --copy-from local:user/api-token"
    )]
    Set(SetArgs),
    /// Print a stored secret value on stdout
    #[command(
        after_help = "Examples:\n  stackly secret reveal local:user/api-token\n  stackly secret reveal local:user/api-token --json"
    )]
    Reveal(RevealArgs),
    /// Delete a secret reference and its stored value; a reference that a
    /// resource still uses is refused
    #[command(
        after_help = "Examples:\n  stackly secret delete local:user/api-token\n  stackly secret delete local:user/api-token --expected-revision 3 --json"
    )]
    Delete(DeleteArgs),
}

impl SecretCommand {
    /// Runs the verb and returns the exit code.
    pub async fn run(self, cx: &mut Context) -> u8 {
        match self {
            Self::List(args) => execute(args, cx).await,
            Self::Status(args) => execute(args, cx).await,
            Self::Set(args) => execute(args, cx).await,
            Self::Reveal(args) => execute(args, cx).await,
            Self::Delete(args) => execute(args, cx).await,
        }
    }
}

/// The time limit of a call that may wait on a Keychain dialog: only an
/// interactive CLI lets the daemon show one (spec 0011, *Input aman CLI*).
fn keychain_spec(cx: &Context) -> CallSpec {
    CallSpec {
        timeout: if cx.prompt_allowed() {
            RequestTimeout::Interactive
        } else {
            RequestTimeout::Control
        },
        idempotency_key: None,
    }
}

/// `stackly secret list` takes no arguments of its own.
#[derive(Debug, Args)]
pub struct ListArgs {}

impl RunCommand for ListArgs {
    type Output = ListSecretsReply;

    async fn run(self, cx: &mut Context) -> CommandOutput<ListSecretsReply> {
        let params = ListSecretsParams {
            owner_kind: None,
            status: None,
        };
        match cx.call(Call::SecretList(params), CallSpec::control()).await {
            Ok(Reply::SecretList(reply)) => CommandOutput::ok(reply),
            Ok(_) => CommandOutput::failed(CliFailure::unexpected_reply(None)),
            Err(failure) => CommandOutput::failed(failure),
        }
    }
}

/// `stackly secret status <reference>`.
#[derive(Debug, Args)]
pub struct StatusArgs {
    /// The secret reference, such as local:user/api-token
    #[arg(value_name = "REFERENCE", value_parser = args::secret_reference)]
    pub reference: String,
}

impl RunCommand for StatusArgs {
    type Output = SecretStatusReply;

    async fn run(self, cx: &mut Context) -> CommandOutput<SecretStatusReply> {
        let params = SecretStatusParams {
            reference: self.reference,
        };
        match cx
            .call(Call::SecretStatus(params), CallSpec::control())
            .await
        {
            Ok(Reply::SecretStatus(reply)) => CommandOutput::ok(*reply),
            Ok(_) => CommandOutput::failed(CliFailure::unexpected_reply(None)),
            Err(failure) => CommandOutput::failed(failure),
        }
    }
}

/// `stackly secret set <reference>`. There is no flag that takes the value
/// itself.
#[derive(Debug, Args)]
pub struct SetArgs {
    /// The secret reference, such as local:user/api-token
    #[arg(value_name = "REFERENCE", value_parser = args::secret_reference)]
    pub reference: String,
    /// Where the value comes from; without a flag it is asked on the
    /// terminal
    #[command(flatten)]
    pub source: SourceArgs,
    /// Replace the value of a reference that exists; fail when it does not
    #[arg(long, conflicts_with = "upsert")]
    pub replace: bool,
    /// Create the reference, or replace its value when it exists
    #[arg(long)]
    pub upsert: bool,
    /// Only act when the reference is still at this revision
    #[arg(long, value_name = "N", value_parser = args::revision)]
    pub expected_revision: Option<Revision>,
    /// Replace a value that only changes the stored copy, without asking
    #[arg(long)]
    pub confirm_stored_only: bool,
}

impl SetArgs {
    fn mode(&self) -> SecretMode {
        if self.replace {
            SecretMode::Replace
        } else if self.upsert {
            SecretMode::Upsert
        } else {
            SecretMode::Create
        }
    }
}

impl RunCommand for SetArgs {
    type Output = SetSecretReply;

    async fn run(self, cx: &mut Context) -> CommandOutput<SetSecretReply> {
        let source = self.source.source();
        let input =
            match secret_input::read_input(cx, &source, &self.reference, &mut Tty, &|name| {
                std::env::var_os(name)
            }) {
                Ok(input) => input,
                Err(failure) => return CommandOutput::failed(failure),
            };
        let approval = Approval {
            given: self.confirm_stored_only,
            flag: CONFIRM_FLAG,
        };
        let mut confirm_stored_only = approval.given;
        loop {
            let params = SetSecretParams {
                reference: self.reference.clone(),
                input: input.clone(),
                mode: self.mode(),
                expected_revision: self.expected_revision,
                confirm_stored_only,
            };
            let spec = keychain_spec(cx);
            match cx.call(Call::SecretSet(params), spec).await {
                Ok(Reply::SecretSet(reply)) => return CommandOutput::ok(*reply),
                Ok(_) => return CommandOutput::failed(CliFailure::unexpected_reply(None)),
                Err(failure)
                    if !confirm_stored_only
                        && failure.body.is(&secret_codes::CONFIRMATION_REQUIRED) =>
                {
                    match ask_stored_only(cx, &self.reference, &failure.body, approval) {
                        Ok(Confirmation::Confirmed) => confirm_stored_only = true,
                        Ok(Confirmation::LeftToDaemon) => {
                            return CommandOutput::failed(failure);
                        }
                        Err(declined) => return CommandOutput::failed(declined),
                    }
                }
                Err(failure) => return CommandOutput::failed(failure),
            }
        }
    }
}

/// Lists the effects the daemon named and asks whether to go on. A
/// noninteractive CLI leaves the daemon's own
/// `secret.confirmation_required` as the answer.
fn ask_stored_only(
    cx: &mut Context,
    reference: &str,
    body: &ErrorBody,
    approval: Approval,
) -> Result<Confirmation, CliFailure> {
    if cx.prompt_allowed() {
        let sink = &mut cx.output.stderr;
        for item in body.items() {
            let field = |key: &str| item.field(key).unwrap_or("?");
            sink.line(&format!(
                "  {} {}, field {}: only the stored copy changes",
                field("resourceKind"),
                field("resourceId"),
                field("field")
            ));
        }
    }
    Confirm::ask(
        cx,
        ConfirmRequest {
            role: ConfirmRole::Standard,
            question: &format!("replace {reference}?"),
            consequence: "the uses above keep running with the old value until they are started again",
        },
        approval,
        OnNoninteractive::LetDaemonDecide,
    )
}

/// `stackly secret reveal <reference>`.
#[derive(Debug, Args)]
pub struct RevealArgs {
    /// The secret reference, such as local:user/api-token
    #[arg(value_name = "REFERENCE", value_parser = args::secret_reference)]
    pub reference: String,
}

impl RunCommand for RevealArgs {
    type Output = RevealSecretReply;

    async fn run(self, cx: &mut Context) -> CommandOutput<RevealSecretReply> {
        let params = RevealSecretParams {
            reference: self.reference,
            purpose: RevealPurpose::Show,
        };
        let spec = keychain_spec(cx);
        match cx.call(Call::SecretReveal(params), spec).await {
            Ok(Reply::SecretReveal(reply)) => CommandOutput::ok(reply),
            Ok(_) => CommandOutput::failed(CliFailure::unexpected_reply(None)),
            Err(failure) => CommandOutput::failed(failure),
        }
    }
}

/// `stackly secret delete <reference>`.
#[derive(Debug, Args)]
pub struct DeleteArgs {
    /// The secret reference, such as local:user/api-token
    #[arg(value_name = "REFERENCE", value_parser = args::secret_reference)]
    pub reference: String,
    /// Only delete when the reference is still at this revision
    #[arg(long, value_name = "N", value_parser = args::revision)]
    pub expected_revision: Option<Revision>,
}

impl RunCommand for DeleteArgs {
    type Output = DeleteSecretReply;

    async fn run(self, cx: &mut Context) -> CommandOutput<DeleteSecretReply> {
        let params = DeleteSecretParams {
            reference: self.reference,
            expected_revision: self.expected_revision,
        };
        let spec = keychain_spec(cx);
        match cx.call(Call::SecretDelete(params), spec).await {
            Ok(Reply::SecretDelete(reply)) => CommandOutput::ok(reply),
            Ok(_) => CommandOutput::failed(CliFailure::unexpected_reply(None)),
            Err(failure) => CommandOutput::failed(failure),
        }
    }
}

const fn status_tone(status: SecretStatus) -> Tone {
    match status {
        SecretStatus::Present => Tone::Positive,
        SecretStatus::Missing | SecretStatus::Unknown => Tone::Caution,
        SecretStatus::Unrecognized => Tone::Neutral,
    }
}

fn status_cell(status: SecretStatus) -> Cell {
    Cell::toned(&wire_name(&status), status_tone(status))
}

fn store_text(store: &SecretStoreStatus) -> String {
    let mut text = format!("{}, {}", wire_name(&store.backend), wire_name(&store.state));
    if let Some(target) = &store.target {
        text.push_str(&format!(" ({})", wire_name(target)));
    }
    text
}

fn summary_rows(out: &HumanWriter, summary: &SecretRefSummary) -> Vec<(&'static str, Cell)> {
    let mut rows = vec![
        ("Reference", Cell::text(&summary.reference)),
        ("Status", status_cell(summary.status)),
    ];
    if let Some(observed) = summary.status_observed_at {
        rows.push(("Checked", Cell::fixed(out.time_and_age(observed))));
    }
    rows.push(("Owner", Cell::fixed(wire_name(&summary.owner_kind))));
    rows.push(("Purpose", Cell::text(&summary.purpose)));
    if let Some(origin) = &summary.value_origin {
        rows.push(("Origin", Cell::fixed(wire_name(origin))));
    }
    if let Some(updated) = summary.value_updated_at {
        rows.push(("Updated", Cell::fixed(out.time_and_age(updated))));
    }
    rows.push(("Revision", Cell::fixed(summary.revision.get().to_string())));
    for (index, usage) in summary.used_by.iter().enumerate() {
        let label = if index == 0 { "Used by" } else { "" };
        rows.push((
            label,
            Cell::text(&format!(
                "{} {} {}",
                wire_name(&usage.resource_kind),
                usage.resource_id,
                usage.field
            )),
        ));
    }
    rows
}

impl Render for ListSecretsReply {
    fn render(&self, out: &mut HumanWriter) {
        if self.items.is_empty() {
            out.line("no secret references");
        } else {
            let rows: Vec<Vec<Cell>> = self
                .items
                .iter()
                .map(|item| {
                    vec![
                        Cell::text(&item.reference),
                        status_cell(item.status),
                        Cell::fixed(wire_name(&item.owner_kind)),
                        item.value_origin.as_ref().map_or_else(
                            || Cell::fixed(""),
                            |origin| Cell::fixed(wire_name(origin)),
                        ),
                        item.value_updated_at
                            .map_or_else(|| Cell::fixed(""), |at| Cell::fixed(out.age(at))),
                        Cell::fixed(item.used_by.len().to_string()),
                        Cell::fixed(item.revision.get().to_string()),
                    ]
                })
                .collect();
            out.table(
                &[
                    "REFERENCE",
                    "STATUS",
                    "OWNER",
                    "ORIGIN",
                    "UPDATED",
                    "USES",
                    "REVISION",
                ],
                &rows,
            );
            out.line("");
        }
        out.kv(&[("Store", Cell::fixed(store_text(&self.store)))]);
    }
}

impl Render for SecretStatusReply {
    fn render(&self, out: &mut HumanWriter) {
        let mut rows = summary_rows(out, &self.summary);
        rows.push(("Store", Cell::fixed(store_text(&self.store))));
        out.kv(&rows);
    }
}

impl Render for SetSecretReply {
    fn render(&self, out: &mut HumanWriter) {
        let mut rows = summary_rows(out, &self.summary);
        for (index, warning) in self.warnings.iter().enumerate() {
            let text = match warning {
                SecretWarning::ShortValueNotScrubbed => {
                    "the value is short, so logs cannot be scrubbed of it everywhere"
                }
                SecretWarning::Unrecognized => {
                    "the daemon sent a warning this stackly does not know"
                }
            };
            let label = if index == 0 { "Warnings" } else { "" };
            rows.push((label, Cell::toned(text, Tone::Caution)));
        }
        for (index, effect) in self.effects.iter().enumerate() {
            let label = if index == 0 { "Effects" } else { "" };
            rows.push((
                label,
                Cell::text(&format!(
                    "{} {} {}: {}",
                    wire_name(&effect.resource_kind),
                    effect.resource_id,
                    effect.field,
                    wire_name(&effect.effect)
                )),
            ));
        }
        out.kv(&rows);
    }
}

impl Render for RevealSecretReply {
    fn render(&self, out: &mut HumanWriter) {
        out.line(self.value.expose());
    }
}

impl Render for DeleteSecretReply {
    fn render(&self, out: &mut HumanWriter) {
        out.kv(&[("Deleted", Cell::text(&self.reference))]);
    }
}
