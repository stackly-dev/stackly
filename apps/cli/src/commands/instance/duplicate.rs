//! `stackly instance duplicate` (spec 0024 AC-11): the settings of an
//! instance as the draft of a new one.
//!
//! The command resolves the source like `inspect`, asks the daemon for the
//! duplicate form, applies the flags on top of its draft, and then sends
//! `instance.preflight` (`--dry-run`) or `instance.create` exactly as
//! `stackly instance create` does. The CLI judges nothing of the draft: the
//! daemon does, once, for the sheet, for `--dry-run`, and for create alike.
//! The runtime and the provider revision are always the ones of the source,
//! so there is no flag to change them.

use clap::{ArgAction, Args};
use serde::Serialize;
use stackly_protocol::instance::{
    CreateInstanceParams, DuplicateForm, EndpointRequest, GetDuplicateFormParams,
    PreflightCreateParams, SecretFieldInput, SecretPlan, SecretSource as PlanSource,
    SettingsDocument,
};
use stackly_protocol::secret::SecretInput;
use stackly_protocol::{Call, Reply};
use uuid::Uuid;

use super::create::{
    CreateOutput, Created, DryRun, extract, first_error, latest_sources, plan_source, read_secrets,
    start_after, typed,
};
use super::resolve::InstanceTarget;
use super::{Verb, conclude};
use crate::args::{PortArg, SettingArg, SettingJsonArg, SettingsFileArg};
use crate::commands::RunCommand;
use crate::context::Context;
use crate::failure::CliFailure;
use crate::output::CommandOutput;
use crate::output::escape::escape_display;
use crate::output::human::{HumanWriter, Render};
use crate::secret_input::{self, SecretSource};
use crate::session::CallSpec;
use crate::wait::AsyncArgs;

/// Where one `--secret` flag of `duplicate` gets its value: any source of
/// `create`, or `copy`, the reference the source uses for the field.
#[derive(Debug, Clone)]
pub enum DuplicateSecret {
    /// `copy`: a copy of the value behind the reference the source uses.
    CopyOfSource,
    /// Any of the sources `create` takes.
    Source(SecretSource),
}

/// `stackly instance duplicate`.
#[derive(Debug, Args)]
pub struct DuplicateArgs {
    /// The instance to copy the settings of: its name, or its whole ID
    #[arg(value_name = "SOURCE")]
    pub source: InstanceTarget,
    /// The unique name of the new instance; the first free <source>-copy by default
    #[arg(value_name = "NAME")]
    pub name: Option<String>,
    /// The name to show
    #[arg(long, value_name = "TEXT")]
    pub display_name: Option<String>,
    /// A port for an endpoint: <endpoint>=<port> or <endpoint>=auto; every other endpoint asks for auto
    #[arg(long = "port", value_name = "ENDPOINT=PORT", action = ArgAction::Append)]
    pub ports: Vec<PortArg>,
    /// A settings field over the settings of the source, read as the kind the form says; the last one for a field wins
    #[arg(long = "set", value_name = "FIELD=VALUE", action = ArgAction::Append)]
    pub settings: Vec<SettingArg>,
    /// A settings field with a JSON value, such as limits=[{"name":"a"}]; it wins over --set for the same field
    #[arg(long = "set-json", value_name = "FIELD=JSON", action = ArgAction::Append)]
    pub settings_json: Vec<SettingJsonArg>,
    /// A JSON file with settings that replace the same fields of the source; --set and --set-json win over it
    #[arg(long, value_name = "PATH.JSON")]
    pub settings_file: Option<SettingsFileArg>,
    /// A secret field and where its value comes from: <field>=<generate|copy|prompt|stdin|file:PATH|env:NAME|copy:REF|bind:REF>; a field left out is generated, and copy takes the reference the source uses
    #[arg(
        long = "secret",
        value_name = "FIELD=SOURCE",
        value_parser = field_source,
        action = ArgAction::Append
    )]
    pub secrets: Vec<(String, DuplicateSecret)>,
    /// Start the instance when the create has succeeded; waits for the create, and with --wait for the start too
    #[arg(long)]
    pub start: bool,
    /// Only judge the draft: print the preflight and exit 0 when it is ready, without creating anything
    #[arg(long)]
    pub dry_run: bool,
    /// `--wait`, `--timeout`, and `--idempotency-key`.
    #[command(flatten)]
    pub run: AsyncArgs,
}

/// Reads `<field>=<source>`: `copy` for the reference of the source, else
/// a source of `create`.
fn field_source(text: &str) -> Result<(String, DuplicateSecret), String> {
    if let Some((field, "copy")) = text.split_once('=')
        && !field.is_empty()
        && field.bytes().all(|b| b.is_ascii_graphic())
    {
        return Ok((field.to_owned(), DuplicateSecret::CopyOfSource));
    }
    secret_input::field_source(text).map(|(field, source)| (field, DuplicateSecret::Source(source)))
}

/// The instance the draft was made from.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceInfo {
    /// The ID of the source.
    pub id: Uuid,
    /// Its name.
    pub name: String,
}

/// The `result` of `duplicate`: the result of `create`, and the source.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Duplicated {
    /// The preflight of `--dry-run`, or the create and the start.
    #[serde(flatten)]
    pub result: CreateOutput,
    /// The instance the settings were copied from.
    pub source: SourceInfo,
}

impl Render for Duplicated {
    fn render(&self, out: &mut HumanWriter) {
        self.result.render(out);
    }
}

impl DuplicateArgs {
    /// The secret flags with `copy` read as the reference the source uses
    /// for the field, the last flag for a field winning.
    ///
    /// # Errors
    ///
    /// `cli.usage` when `copy` names a field the source has no reference
    /// for.
    fn resolved_secrets(
        &self,
        form: &DuplicateForm,
    ) -> Result<Vec<(String, SecretSource)>, CliFailure> {
        self.secrets
            .iter()
            .map(|(field, flag)| {
                let source = match flag {
                    DuplicateSecret::Source(source) => source.clone(),
                    DuplicateSecret::CopyOfSource => form
                        .secrets
                        .iter()
                        .find(|secret| secret.field == *field)
                        .and_then(|secret| secret.source_reference.clone())
                        .map(SecretSource::CopyFrom)
                        .ok_or_else(|| {
                            CliFailure::usage(
                                "secret_copy_without_reference",
                                &format!(
                                    "--secret {}=copy: {} uses no reference for that field",
                                    escape_display(field),
                                    escape_display(&form.source.name)
                                ),
                            )
                        })?,
                };
                Ok((field.clone(), source))
            })
            .collect()
    }

    /// The endpoints of the draft with every `--port` over the request of
    /// the same endpoint.
    fn endpoints(&self, form: &DuplicateForm) -> Vec<EndpointRequest> {
        let mut endpoints = form.draft.endpoints.clone();
        for port in &self.ports {
            match endpoints
                .iter_mut()
                .find(|known| known.name == port.endpoint)
            {
                Some(slot) => slot.port = port.port,
                None => endpoints.push(EndpointRequest {
                    name: port.endpoint.clone(),
                    port: port.port,
                }),
            }
        }
        endpoints
    }

    /// The settings of the source, then the file over them, then `--set`
    /// read as the kind the form says, then `--set-json`.
    fn settings(&self, form: &DuplicateForm) -> SettingsDocument {
        let mut map = form.draft.settings.as_map().clone();
        if let Some(file) = &self.settings_file {
            map.extend(file.0.clone());
        }
        for setting in &self.settings {
            let kind = form
                .form
                .fields
                .iter()
                .find(|field| field.key == setting.field)
                .map(|field| field.kind);
            map.insert(setting.field.clone(), typed(kind, &setting.value));
        }
        for setting in &self.settings_json {
            map.insert(setting.field.clone(), setting.value.clone());
        }
        SettingsDocument::new(map)
    }

    /// The plan of each secret field of the draft, and of each flag for a
    /// field the draft does not list: where the value comes from, never
    /// the value.
    fn plans(form: &DuplicateForm, sources: &[(&str, &SecretSource)]) -> Vec<SecretPlan> {
        let mut plans: Vec<SecretPlan> = form
            .draft
            .secrets
            .iter()
            .map(|plan| SecretPlan {
                field: plan.field.clone(),
                source: sources
                    .iter()
                    .find(|(field, _)| *field == plan.field)
                    .map_or(PlanSource::Generate, |(_, source)| plan_source(source)),
            })
            .collect();
        for (field, source) in sources {
            if !plans.iter().any(|plan| plan.field == *field) {
                plans.push(SecretPlan {
                    field: (*field).to_owned(),
                    source: plan_source(source),
                });
            }
        }
        plans
    }

    /// The inputs of `instance.create`: the value of each flag, and a
    /// generated secret for every other secret field of the draft.
    fn inputs(
        cx: &mut Context,
        form: &DuplicateForm,
        sources: &[(&str, &SecretSource)],
    ) -> Result<Vec<SecretFieldInput>, CliFailure> {
        let mut read = read_secrets(cx, sources)?;
        let mut inputs = Vec::new();
        for plan in &form.draft.secrets {
            match read.iter().position(|input| input.field == plan.field) {
                Some(index) => inputs.push(read.remove(index)),
                None => inputs.push(SecretFieldInput {
                    field: plan.field.clone(),
                    input: SecretInput::Generate,
                }),
            }
        }
        inputs.extend(read);
        Ok(inputs)
    }

    async fn dry_run(
        &self,
        cx: &mut Context,
        form: &DuplicateForm,
        sources: &[(&str, &SecretSource)],
    ) -> CommandOutput<CreateOutput> {
        let call = Call::InstancePreflight(PreflightCreateParams {
            name: self.name.clone().unwrap_or_else(|| form.draft.name.clone()),
            display_name: self.display_name.clone(),
            provider: form.draft.provider.clone(),
            runtime_id: form.draft.runtime_id,
            endpoints: self.endpoints(form),
            settings: self.settings(form),
            secrets: Self::plans(form, sources),
            start: self.start,
        });
        let preflight = match cx.call(call, CallSpec::control()).await {
            Ok(Reply::InstancePreflight(preflight)) => *preflight,
            Ok(_) => return CommandOutput::failed(CliFailure::unexpected_reply(None)),
            Err(failure) => return CommandOutput::failed(failure),
        };
        let failure = first_error(&preflight);
        let output = DryRun { preflight };
        match failure {
            Some(failure) => CommandOutput::partial(CreateOutput::DryRun(output), failure),
            None => CommandOutput::ok(CreateOutput::DryRun(output)),
        }
    }

    async fn create(
        &self,
        cx: &mut Context,
        form: &DuplicateForm,
        sources: &[(&str, &SecretSource)],
    ) -> CommandOutput<CreateOutput> {
        let secrets = match Self::inputs(cx, form, sources) {
            Ok(secrets) => secrets,
            Err(failure) => return CommandOutput::failed(failure),
        };
        let name = self.name.clone().unwrap_or_else(|| form.draft.name.clone());
        let params = CreateInstanceParams {
            name: name.clone(),
            display_name: self.display_name.clone(),
            provider: form.draft.provider.clone(),
            runtime_id: form.draft.runtime_id,
            endpoints: self.endpoints(form),
            settings: self.settings(form),
            secrets,
        };
        // `--start` needs the create to end first, whatever `--wait` says.
        let mut run = self.run.clone();
        if self.start {
            run.wait.wait = true;
        }
        let submission = match cx.submit(Call::InstanceCreate(params), &run, extract).await {
            Ok(submission) => submission,
            Err(failure) => return CommandOutput::failed(failure),
        };
        let mut output = conclude(cx, submission, Verb::Create { name }, None).await;
        let (start, start_failure) = match (&output.result, &output.failure) {
            (Some(created), None) if self.start && created.succeeded() => {
                start_after(cx, &self.run, created).await
            }
            _ => (None, None),
        };
        CommandOutput {
            result: output
                .result
                .take()
                .map(|action| CreateOutput::Created(Box::new(Created { action, start }))),
            failure: output.failure.take().or(start_failure),
            warnings: output.warnings,
            hints: output.hints,
        }
    }
}

impl RunCommand for DuplicateArgs {
    type Output = Duplicated;

    async fn run(self, cx: &mut Context) -> CommandOutput<Duplicated> {
        let id = match self.source.resolve(cx).await {
            Ok(id) => id,
            Err(failure) => return CommandOutput::failed(failure),
        };
        let call = Call::InstanceGetDuplicateForm(GetDuplicateFormParams { instance_id: id });
        let form = match cx.call(call, CallSpec::control()).await {
            Ok(Reply::InstanceGetDuplicateForm(form)) => *form,
            Ok(_) => return CommandOutput::failed(CliFailure::unexpected_reply(None)),
            Err(failure) => return CommandOutput::failed(failure),
        };
        let source = SourceInfo {
            id: form.source.instance_id,
            name: form.source.name.clone(),
        };
        if !cx.output.quiet {
            cx.output.stderr.line(&format!(
                "Duplicating settings of {}; no data is copied.",
                escape_display(&source.name)
            ));
        }
        let resolved = match self.resolved_secrets(&form) {
            Ok(resolved) => resolved,
            Err(failure) => return CommandOutput::failed(failure),
        };
        let sources = latest_sources(&resolved);
        let output = if self.dry_run {
            self.dry_run(cx, &form, &sources).await
        } else {
            self.create(cx, &form, &sources).await
        };
        CommandOutput {
            result: output.result.map(|result| Duplicated { result, source }),
            failure: output.failure,
            warnings: output.warnings,
            hints: output.hints,
        }
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

    #[derive(Parser)]
    struct Wrapper {
        #[command(flatten)]
        args: DuplicateArgs,
    }

    fn parse(args: &[&str]) -> Result<DuplicateArgs, clap::Error> {
        Wrapper::try_parse_from(std::iter::once("duplicate").chain(args.iter().copied()))
            .map(|wrapper| wrapper.args)
    }

    #[test]
    fn the_source_and_the_optional_name_are_positional() {
        let args = parse(&["pg17-dev", "pg17-test"]).expect("two names parse");
        assert_eq!(args.source, InstanceTarget::Name("pg17-dev".to_owned()));
        assert_eq!(args.name.as_deref(), Some("pg17-test"));
        let args = parse(&["pg17-dev"]).expect("the name is optional");
        assert_eq!(args.name, None);
    }

    #[test]
    fn no_flag_picks_the_runtime_or_the_provider() {
        for flag in [
            ["--runtime", "postgresql@17"],
            ["--provider", "official/postgresql"],
            ["--revision", "1.0.0"],
        ] {
            let error = parse(&["pg17-dev", flag[0], flag[1]]).expect_err("refused");
            assert_eq!(
                error.kind(),
                clap::error::ErrorKind::UnknownArgument,
                "{flag:?}"
            );
        }
    }

    #[test]
    fn copy_is_a_secret_source_besides_the_sources_of_create() {
        let args = parse(&[
            "pg17-dev",
            "--secret",
            "superuserPassword=copy",
            "--secret",
            "token=generate",
            "--secret",
            "other=copy:local:instance/0192f3a5-0000-7000-8000-000000000001/other",
        ])
        .expect("the secret flags parse");
        assert!(matches!(args.secrets[0].1, DuplicateSecret::CopyOfSource));
        assert!(matches!(
            args.secrets[1].1,
            DuplicateSecret::Source(SecretSource::Generate)
        ));
        assert!(matches!(
            args.secrets[2].1,
            DuplicateSecret::Source(SecretSource::CopyFrom(_))
        ));
        assert!(parse(&["pg17-dev", "--secret", "field=nonsense"]).is_err());
    }
}
