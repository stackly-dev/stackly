//! `stackly instance config get|validate|apply|rollback|keep-revision`
//! (spec 0029, AC-17, *CLI*).
//!
//! The CLI judges nothing of a change: it reads the config once (`get`),
//! builds the whole proposed settings document from the flags, and sends it
//! with the revision it read as `baseRevision`. The daemon refuses what a
//! rule decides, with the code and the field errors the app shows.
//! `rollback` and `keep-revision` ask first, because the first writes a
//! revision over the running one and the second throws away edits made
//! outside Stackly.

use clap::{ArgAction, Args, Subcommand};
use stackly_protocol::codes::instance as instance_codes;
use stackly_protocol::instance::config::{
    ApplyConfigParams, ConfigApplyAccepted, ConfigChangeRequest, ConfigValidation, EffectiveFile,
    GetConfigParams, InstanceConfigView, KeepRevisionParams, ValidateConfigParams,
};
use stackly_protocol::instance::{PortRequest, SettingsDocument};
use stackly_protocol::{Call, ConfigRevisionNumber, ErrorBody, Reply};
use uuid::Uuid;

use super::create::typed;
use super::resolve::TargetArg;
use super::{InstanceAction, Verb, conclude};
use crate::args::{SettingArg, SettingJsonArg};
use crate::commands::{RunCommand, execute};
use crate::confirm::{Approval, Confirm, ConfirmRequest, ConfirmRole, OnNoninteractive};
use crate::context::Context;
use crate::failure::CliFailure;
use crate::output::escape::escape_display;
use crate::output::human::{Cell, HumanWriter, Render, wire_name};
use crate::output::{CommandOutput, OutputMode};
use crate::session::CallSpec;
use crate::wait::{Accepted, AsyncArgs};

/// The verbs of `stackly instance config`.
#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    /// Show the settings, revision, history, and drift of an instance
    #[command(
        after_help = "Examples:\n  stackly instance config get orders-db\n  stackly instance config get orders-db --json\n  stackly instance config get orders-db --file postgresql.conf"
    )]
    Get(GetArgs),
    /// Judge a change without any effect: the diff, the impact, and the field errors
    #[command(
        after_help = "Examples:\n  stackly instance config validate orders-db --set maxConnections=200\n  stackly instance config validate orders-db --from-revision 1 --json"
    )]
    Validate(ValidateArgs),
    /// Write a change as the next revision and make it take effect
    #[command(
        after_help = "Examples:\n  stackly instance config apply orders-db --set maxConnections=200 --wait\n  stackly instance config apply orders-db --from-revision 1 --wait\n  stackly instance config apply orders-db --set logStatements=ddl --dry-run --json"
    )]
    Apply(ApplyArgs),
    /// Go back to the last known good configuration, as a new revision
    #[command(after_help = "Examples:\n  stackly instance config rollback orders-db --yes --wait")]
    Rollback(RollbackArgs),
    /// Write the config files of the active revision back over edits made outside Stackly
    #[command(
        name = "keep-revision",
        after_help = "Examples:\n  stackly instance config keep-revision orders-db --yes --wait"
    )]
    KeepRevision(KeepRevisionArgs),
}

impl ConfigCommand {
    /// Runs the verb and returns the exit code.
    pub async fn run(self, cx: &mut Context) -> u8 {
        match self {
            Self::Get(args) => execute(args, cx).await,
            Self::Validate(args) => execute(args, cx).await,
            Self::Apply(args) => execute(args, cx).await,
            Self::Rollback(args) => execute(args, cx).await,
            Self::KeepRevision(args) => execute(args, cx).await,
        }
    }
}

/// `stackly instance config get`.
#[derive(Debug, Args)]
pub struct GetArgs {
    /// The instance.
    #[command(flatten)]
    pub instance: TargetArg,
    /// Print one config file, by its path below the config folder
    #[arg(long, value_name = "PATH")]
    pub file: Option<String>,
}

/// The change the flags of `validate` and `apply` describe.
#[derive(Debug, Args)]
pub struct ChangeArgs {
    /// The instance.
    #[command(flatten)]
    pub instance: TargetArg,
    /// A settings field, read as the kind the form says: a number, true or false, or text; the last one for a field wins
    #[arg(long = "set", value_name = "FIELD=VALUE", action = ArgAction::Append)]
    pub settings: Vec<SettingArg>,
    /// A settings field with a JSON value, such as routes=[{"name":"a"}]; it wins over --set for the same field
    #[arg(long = "set-json", value_name = "FIELD=JSON", action = ArgAction::Append)]
    pub settings_json: Vec<SettingJsonArg>,
    /// Take a field out, so it falls back to the default of the schema; a field that is required without a default is refused
    #[arg(long = "unset", value_name = "FIELD", action = ArgAction::Append)]
    pub unset: Vec<String>,
    /// Start from the settings and ports of this older revision of the history; --set, --set-json, and --unset change it
    #[arg(long, value_name = "N")]
    pub from_revision: Option<u64>,
}

/// `stackly instance config validate`.
#[derive(Debug, Args)]
pub struct ValidateArgs {
    /// What to judge.
    #[command(flatten)]
    pub change: ChangeArgs,
}

/// `stackly instance config apply`.
#[derive(Debug, Args)]
pub struct ApplyArgs {
    /// What to change.
    #[command(flatten)]
    pub change: ChangeArgs,
    /// Refuse unless this is the active revision; by default the one just read
    #[arg(long, value_name = "N")]
    pub expect_revision: Option<u64>,
    /// Only judge the change, like validate: print the verdict and change nothing
    #[arg(long)]
    pub dry_run: bool,
    /// `--wait`, `--timeout`, and `--idempotency-key`.
    #[command(flatten)]
    pub run: AsyncArgs,
}

/// `stackly instance config rollback`.
#[derive(Debug, Args)]
pub struct RollbackArgs {
    /// The instance.
    #[command(flatten)]
    pub instance: TargetArg,
    /// Confirm without asking; needed without a terminal and with --json
    #[arg(short = 'y', long)]
    pub yes: bool,
    /// `--wait`, `--timeout`, and `--idempotency-key`.
    #[command(flatten)]
    pub run: AsyncArgs,
}

/// `stackly instance config keep-revision`.
#[derive(Debug, Args)]
pub struct KeepRevisionArgs {
    /// The instance.
    #[command(flatten)]
    pub instance: TargetArg,
    /// Confirm without asking; needed without a terminal and with --json
    #[arg(short = 'y', long)]
    pub yes: bool,
    /// `--wait`, `--timeout`, and `--idempotency-key`.
    #[command(flatten)]
    pub run: AsyncArgs,
}

/// The result of `get`: the view, or with `--file` one config file.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(untagged)]
pub enum ConfigGetOutput {
    /// The view.
    View(Box<InstanceConfigView>),
    /// One file.
    File(EffectiveFile),
}

/// The result of `validate` and `apply --dry-run`.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(transparent)]
pub struct ValidateOutput(pub ConfigValidation);

/// The result of `apply`: the verdict of `--dry-run`, or the operation.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(untagged)]
pub enum ApplyOutput {
    /// `--dry-run`.
    DryRun(ValidateOutput),
    /// An apply.
    Applied(Box<InstanceAction>),
}

/// Reads the config of `id`.
pub(super) async fn read_view(
    cx: &mut Context,
    id: Uuid,
) -> Result<InstanceConfigView, CliFailure> {
    let call = Call::InstanceConfigGet(GetConfigParams { instance_id: id });
    match cx.call(call, CallSpec::control()).await? {
        Reply::InstanceConfigGet(view) => Ok(*view),
        _ => Err(CliFailure::unexpected_reply(None)),
    }
}

/// The change the flags describe, over the config that was read.
///
/// `--from-revision` alone is a restore of the whole revision, ports
/// included. With a `--set`, `--set-json`, or `--unset` the settings of
/// that revision (or of the active one) are the start and the ports stay.
pub(super) fn build_change(
    args: &ChangeArgs,
    view: &InstanceConfigView,
) -> Result<ConfigChangeRequest, CliFailure> {
    let edits =
        !args.settings.is_empty() || !args.settings_json.is_empty() || !args.unset.is_empty();
    let start = match args.from_revision {
        Some(number) => {
            let number = ConfigRevisionNumber::new(number)
                .filter(|number| number.get() > 0)
                .ok_or_else(|| CliFailure::usage("from_revision", "a revision starts at 1"))?;
            if !edits {
                return Ok(ConfigChangeRequest::Restore(number));
            }
            let found = view
                .history
                .iter()
                .find(|revision| revision.number == number)
                .ok_or_else(|| {
                    CliFailure::usage(
                        "from_revision",
                        &format!("revision {number} is not in the history"),
                    )
                })?;
            found.values.as_map().clone()
        }
        None => view.revision.values.as_map().clone(),
    };
    let mut map = start;
    for field in &args.unset {
        map.remove(field);
    }
    for setting in &args.settings {
        let kind = view
            .fields
            .iter()
            .find(|field| field.field.key == setting.field)
            .map(|field| field.field.kind);
        map.insert(setting.field.clone(), typed(kind, &setting.value));
    }
    for setting in &args.settings_json {
        map.insert(setting.field.clone(), setting.value.clone());
    }
    Ok(ConfigChangeRequest::Settings(SettingsDocument::new(map)))
}

/// A verdict that is not valid as the error `instance.settings_invalid`
/// with the field errors of the verdict, so the exit code and the codes
/// are the ones `apply` would answer with.
pub(super) fn invalid_failure(verdict: &ConfigValidation) -> Option<CliFailure> {
    if verdict.valid {
        return None;
    }
    let body = ErrorBody::new(
        &instance_codes::SETTINGS_INVALID,
        "the settings break the schema of the package",
    )
    .with_fields(verdict.field_errors.clone());
    Some(CliFailure::daemon(Box::new(body), None))
}

/// The reply of `apply` as what a submitted request holds.
fn extract_apply(reply: Reply) -> Option<Accepted> {
    match reply {
        Reply::InstanceConfigApply(reply) => Some(accepted_apply(&reply)),
        _ => None,
    }
}

/// What a reply of `instance.config.apply` holds.
pub(super) fn accepted_apply(reply: &ConfigApplyAccepted) -> Accepted {
    Accepted {
        operation: reply
            .accepted
            .as_ref()
            .map(|accepted| accepted.operation.clone()),
        replayed: reply
            .accepted
            .as_ref()
            .is_some_and(|accepted| accepted.replayed),
        already_in_state: reply.unchanged,
    }
}

fn extract_keep(reply: Reply) -> Option<Accepted> {
    match reply {
        Reply::InstanceConfigKeepRevision(reply) => Some(super::accepted(&reply)),
        _ => None,
    }
}

/// Confirms a change that gives something up: `--yes` passes; with
/// `--json` the noninteractive path holds even at a terminal, so `--yes` is
/// needed (AC-17).
fn confirm(
    cx: &mut Context,
    yes: bool,
    question: &str,
    consequence: &str,
) -> Result<(), CliFailure> {
    let approval = Approval {
        given: yes,
        flag: "--yes",
    };
    if !yes && cx.output.mode == OutputMode::Json {
        return Err(CliFailure::confirmation_required(approval.flag));
    }
    let request = ConfirmRequest {
        role: ConfirmRole::Destructive,
        question,
        consequence,
    };
    Confirm::ask(cx, request, approval, OnNoninteractive::Fail).map(|_| ())
}

/// The target as it was typed, escaped for a question.
fn shown(target: &TargetArg) -> String {
    escape_display(&target.target.to_string()).into_owned()
}

impl RunCommand for GetArgs {
    type Output = ConfigGetOutput;

    async fn run(self, cx: &mut Context) -> CommandOutput<ConfigGetOutput> {
        let id = match self.instance.target.resolve(cx).await {
            Ok(id) => id,
            Err(failure) => return CommandOutput::failed(failure),
        };
        let view = match read_view(cx, id).await {
            Ok(view) => view,
            Err(failure) => return CommandOutput::failed(failure),
        };
        let Some(path) = self.file else {
            return CommandOutput::ok(ConfigGetOutput::View(Box::new(view)));
        };
        match view.effective_files.iter().find(|file| file.path == path) {
            Some(file) => CommandOutput::ok(ConfigGetOutput::File(file.clone())),
            None => CommandOutput::failed(CliFailure::usage(
                "file",
                &format!("the instance has no config file {path}"),
            )),
        }
    }
}

/// Judges `change` for `id` against `base`.
async fn validate(
    cx: &mut Context,
    id: Uuid,
    base: ConfigRevisionNumber,
    change: ConfigChangeRequest,
) -> CommandOutput<ValidateOutput> {
    let call = Call::InstanceConfigValidate(ValidateConfigParams {
        instance_id: id,
        base_revision: base,
        change,
    });
    let verdict = match cx.call(call, CallSpec::control()).await {
        Ok(Reply::InstanceConfigValidate(verdict)) => *verdict,
        Ok(_) => return CommandOutput::failed(CliFailure::unexpected_reply(None)),
        Err(failure) => return CommandOutput::failed(failure),
    };
    let failure = invalid_failure(&verdict);
    let output = ValidateOutput(verdict);
    match failure {
        Some(failure) => CommandOutput::partial(output, failure),
        None => CommandOutput::ok(output),
    }
}

impl RunCommand for ValidateArgs {
    type Output = ValidateOutput;

    async fn run(self, cx: &mut Context) -> CommandOutput<ValidateOutput> {
        let id = match self.change.instance.target.resolve(cx).await {
            Ok(id) => id,
            Err(failure) => return CommandOutput::failed(failure),
        };
        let view = match read_view(cx, id).await {
            Ok(view) => view,
            Err(failure) => return CommandOutput::failed(failure),
        };
        let change = match build_change(&self.change, &view) {
            Ok(change) => change,
            Err(failure) => return CommandOutput::failed(failure),
        };
        validate(cx, id, view.revision.number, change).await
    }
}

/// The base revision of an apply: the one the caller expects, else the
/// one just read.
fn base_of(
    expect: Option<u64>,
    view: &InstanceConfigView,
) -> Result<ConfigRevisionNumber, CliFailure> {
    match expect {
        None => Ok(view.revision.number),
        Some(number) => ConfigRevisionNumber::new(number)
            .filter(|number| number.get() > 0)
            .ok_or_else(|| CliFailure::usage("expect_revision", "a revision starts at 1")),
    }
}

impl RunCommand for ApplyArgs {
    type Output = ApplyOutput;

    async fn run(self, cx: &mut Context) -> CommandOutput<ApplyOutput> {
        let id = match self.change.instance.target.resolve(cx).await {
            Ok(id) => id,
            Err(failure) => return CommandOutput::failed(failure),
        };
        let view = match read_view(cx, id).await {
            Ok(view) => view,
            Err(failure) => return CommandOutput::failed(failure),
        };
        let change = match build_change(&self.change, &view) {
            Ok(change) => change,
            Err(failure) => return CommandOutput::failed(failure),
        };
        let base = match base_of(self.expect_revision, &view) {
            Ok(base) => base,
            Err(failure) => return CommandOutput::failed(failure),
        };
        if self.dry_run {
            let output = validate(cx, id, base, change).await;
            return mapped(output, ApplyOutput::DryRun);
        }
        let call = Call::InstanceConfigApply(ApplyConfigParams {
            instance_id: id,
            base_revision: base,
            change,
            expected_revision: None,
        });
        match cx.submit(call, &self.run, extract_apply).await {
            Ok(submission) => {
                let output = conclude(cx, submission, Verb::Apply, Some(id)).await;
                mapped(output, |action| ApplyOutput::Applied(Box::new(action)))
            }
            Err(failure) => CommandOutput::failed(failure),
        }
    }
}

impl RunCommand for RollbackArgs {
    type Output = InstanceAction;

    async fn run(self, cx: &mut Context) -> CommandOutput<InstanceAction> {
        // Asked before anything reaches the daemon, the name included.
        let question = format!(
            "roll back {} to its last known good configuration?",
            shown(&self.instance)
        );
        if let Err(failure) = confirm(
            cx,
            self.yes,
            &question,
            "A new revision is written with the settings and ports of the last known good revision, and a running instance reloads or restarts to use it.",
        ) {
            return CommandOutput::failed(failure);
        }
        let id = match self.instance.target.resolve(cx).await {
            Ok(id) => id,
            Err(failure) => return CommandOutput::failed(failure),
        };
        let view = match read_view(cx, id).await {
            Ok(view) => view,
            Err(failure) => return CommandOutput::failed(failure),
        };
        let call = Call::InstanceConfigApply(ApplyConfigParams {
            instance_id: id,
            base_revision: view.revision.number,
            change: ConfigChangeRequest::Rollback,
            expected_revision: None,
        });
        match cx.submit(call, &self.run, extract_apply).await {
            Ok(submission) => conclude(cx, submission, Verb::Rollback, Some(id)).await,
            Err(failure) => CommandOutput::failed(failure),
        }
    }
}

impl RunCommand for KeepRevisionArgs {
    type Output = InstanceAction;

    async fn run(self, cx: &mut Context) -> CommandOutput<InstanceAction> {
        let question = format!(
            "keep the active revision of {} and overwrite the edited files?",
            shown(&self.instance)
        );
        if let Err(failure) = confirm(
            cx,
            self.yes,
            &question,
            "The files that were changed outside Stackly are written back from the active revision. Those edits are lost.",
        ) {
            return CommandOutput::failed(failure);
        }
        let id = match self.instance.target.resolve(cx).await {
            Ok(id) => id,
            Err(failure) => return CommandOutput::failed(failure),
        };
        let call = Call::InstanceConfigKeepRevision(KeepRevisionParams { instance_id: id });
        match cx.submit(call, &self.run, extract_keep).await {
            Ok(submission) => conclude(cx, submission, Verb::KeepRevision, Some(id)).await,
            Err(failure) => CommandOutput::failed(failure),
        }
    }
}

impl Render for ConfigGetOutput {
    fn render(&self, out: &mut HumanWriter) {
        match self {
            Self::View(view) => render_view(out, view),
            Self::File(file) => {
                for line in file.content.lines() {
                    out.line(&escape_display(line));
                }
            }
        }
    }
}

fn render_view(out: &mut HumanWriter, view: &InstanceConfigView) {
    let good = view
        .last_known_good
        .map_or_else(|| "none yet".to_owned(), |number| number.to_string());
    out.kv(&[
        ("Revision", Cell::fixed(view.revision.number.to_string())),
        ("Origin", Cell::text(&wire_name(&view.revision.origin))),
        ("Last known good", Cell::fixed(good)),
        (
            "Reload",
            Cell::fixed(if view.reload_supported {
                "supported"
            } else {
                "restart only"
            }),
        ),
    ]);
    if let Some(drift) = &view.drift {
        let files: Vec<&str> = drift.files.iter().map(|file| file.path.as_str()).collect();
        out.line("");
        out.line(&format!(
            "changed outside Stackly: {} (stackly instance config keep-revision writes them back)",
            escape_display(&files.join(", "))
        ));
    }
    out.line("");
    let rows: Vec<Vec<Cell>> = view
        .fields
        .iter()
        .map(|field| {
            let value = view
                .revision
                .values
                .as_map()
                .get(&field.field.key)
                .map_or_else(String::new, |value| match value {
                    serde_json::Value::String(text) => text.clone(),
                    other => other.to_string(),
                });
            vec![
                Cell::text(&field.field.key),
                Cell::text(&value),
                Cell::fixed(field_note(field)),
            ]
        })
        .collect();
    out.table(&["Setting", "Value", "Applies"], &rows);
    if !view.history.is_empty() {
        out.line("");
        let rows: Vec<Vec<Cell>> = view
            .history
            .iter()
            .map(|revision| {
                let mut marks = Vec::new();
                if revision.is_active {
                    marks.push("active");
                }
                if revision.is_last_known_good {
                    marks.push("last known good");
                }
                vec![
                    Cell::fixed(revision.number.to_string()),
                    Cell::text(&wire_name(&revision.origin)),
                    Cell::fixed(out.time(revision.created_at)),
                    Cell::fixed(marks.join(", ")),
                ]
            })
            .collect();
        out.table(&["Revision", "Origin", "Written", "Marks"], &rows);
    }
}

/// How a field takes effect, in words.
fn field_note(field: &stackly_protocol::instance::config::ConfigField) -> String {
    if field.field.secret {
        return "secret, read only".to_owned();
    }
    if field.field.managed {
        return "set by Stackly".to_owned();
    }
    match field.field.apply {
        Some(stackly_protocol::instance::FieldApply::Reload) => "reload".to_owned(),
        Some(stackly_protocol::instance::FieldApply::Restart) => "restart".to_owned(),
        Some(stackly_protocol::instance::FieldApply::ImmutableAfterInit) => "set once".to_owned(),
        _ => String::new(),
    }
}

impl Render for ValidateOutput {
    fn render(&self, out: &mut HumanWriter) {
        let verdict = &self.0;
        if !verdict.valid {
            out.line("invalid");
            for field in &verdict.field_errors {
                out.line(&format!(
                    "  {}: {}",
                    escape_display(field.path()),
                    escape_display(field.message())
                ));
            }
            return;
        }
        if verdict.changes.is_empty() && verdict.endpoint_changes.is_empty() {
            out.line("valid, nothing changes");
            return;
        }
        out.line(&format!("valid, impact {}", wire_name(&verdict.impact)));
        for change in &verdict.changes {
            out.line(&format!(
                "  {}: {} -> {}",
                escape_display(&change.key),
                shown_value(change.old.as_deref()),
                shown_value(change.new.as_deref())
            ));
        }
        for change in &verdict.endpoint_changes {
            out.line(&format!(
                "  port {}: {} -> {}",
                escape_display(&change.name),
                request_text(change.old),
                request_text(change.new)
            ));
        }
    }
}

impl Render for ApplyOutput {
    fn render(&self, out: &mut HumanWriter) {
        match self {
            Self::DryRun(verdict) => verdict.render(out),
            Self::Applied(action) => action.render(out),
        }
    }
}

fn shown_value(value: Option<&str>) -> String {
    value.map_or_else(
        || "(none)".to_owned(),
        |text| escape_display(text).into_owned(),
    )
}

/// A port request the way `--port` takes it: the port number, or `auto`.
/// The change lines of `config validate`, `config apply --dry-run`, and
/// `endpoint set --dry-run` use it.
pub(super) fn request_text(request: PortRequest) -> String {
    match request {
        PortRequest::Fixed(port) => port.to_string(),
        PortRequest::Auto => "auto".to_owned(),
    }
}

/// `output` with its result turned into another type.
pub(super) fn mapped<T, U>(output: CommandOutput<T>, f: impl FnOnce(T) -> U) -> CommandOutput<U> {
    CommandOutput {
        result: output.result.map(f),
        failure: output.failure,
        warnings: output.warnings,
        hints: output.hints,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(result: &impl Render) -> String {
        let mut out = HumanWriter::new(false, jiff::Timestamp::UNIX_EPOCH, jiff::tz::TimeZone::UTC);
        result.render(&mut out);
        out.into_string()
    }

    #[test]
    fn a_field_error_message_is_escaped_like_its_path() {
        let verdict: ConfigValidation = serde_json::from_value(serde_json::json!({
            "valid": false,
            "fieldErrors": [{
                "path": "max\u{1b}",
                "code": "settings.invalid",
                "message": "bad \u{1b}[2Jvalue\u{202e}",
            }],
            "changes": [],
            "endpointChanges": [],
            "impact": "none",
            "preview": [],
        }))
        .unwrap();
        assert_eq!(
            render(&ValidateOutput(verdict)),
            "invalid\n  max\\u{1b}: bad \\u{1b}[2Jvalue\\u{202e}\n"
        );
    }

    #[test]
    fn a_port_change_names_the_old_and_new_requests_like_port_takes_them() {
        let verdict: ConfigValidation = serde_json::from_value(serde_json::json!({
            "valid": true,
            "fieldErrors": [],
            "changes": [{"key": "maxConnections", "old": "100", "new": "200"}],
            "endpointChanges": [
                {
                    "name": "postgres",
                    "old": {"type": "fixed", "content": 5432},
                    "new": {"type": "fixed", "content": 5433},
                },
                {"name": "admin", "old": {"type": "auto"}, "new": {"type": "fixed", "content": 8080}},
            ],
            "impact": "restart",
            "preview": [],
        }))
        .unwrap();
        let shown = "valid, impact restart\n  maxConnections: 100 -> 200\n  port postgres: 5432 -> 5433\n  port admin: auto -> 8080\n";
        assert_eq!(render(&ValidateOutput(verdict.clone())), shown);
        assert_eq!(render(&ApplyOutput::DryRun(ValidateOutput(verdict))), shown);
    }
}
