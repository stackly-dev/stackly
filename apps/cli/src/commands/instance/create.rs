//! `stackly instance create` (spec 0014, AC-5). The flags map to the params
//! of `instance.create` as the table *Pemetaan flag create* says; the CLI
//! checks only the shape of what it was given.

use clap::{ArgAction, Args};
use stackly_protocol::instance::{
    CreateInstanceParams, EndpointRequest, ProviderSelector, SettingsDocument,
};
use stackly_protocol::{Call, Reply};
use uuid::Uuid;

use super::{InstanceAction, Verb, accepted, conclude};
use crate::args::{self, PortArg, SettingArg, SettingJsonArg};
use crate::commands::RunCommand;
use crate::context::Context;
use crate::output::CommandOutput;
use crate::wait::{Accepted, AsyncArgs};

// provisional: F20 (spec 0020 steps 9 and 10 replace the UUID only
// `--runtime`, the required `--provider` and `--revision`, and the
// text only `--set`)
/// `stackly instance create`.
#[derive(Debug, Args)]
pub struct CreateArgs {
    /// The unique instance name
    #[arg(value_name = "NAME")]
    pub name: String,
    /// The installed runtime to run it on, an ID printed by the seed or by runtime listings
    #[arg(long, value_name = "RUNTIME_ID", value_parser = args::runtime_id)]
    pub runtime: Uuid,
    /// The provider to create it from
    #[arg(long, value_name = "NAME")]
    pub provider: String,
    /// The provider revision to pin
    #[arg(long, value_name = "LABEL")]
    pub revision: String,
    /// The name to show
    #[arg(long, value_name = "TEXT")]
    pub display_name: Option<String>,
    /// A port for an endpoint: <endpoint>=<port> or <endpoint>=auto; repeat for more endpoints
    #[arg(long = "port", value_name = "ENDPOINT=PORT", action = ArgAction::Append)]
    pub ports: Vec<PortArg>,
    /// A settings field with a text value; the last one for a field wins
    #[arg(long = "set", value_name = "FIELD=TEXT", action = ArgAction::Append)]
    pub settings: Vec<SettingArg>,
    /// A settings field with a JSON value, such as readyDelayMs=1500; it wins over --set for the same field
    #[arg(long = "set-json", value_name = "FIELD=JSON", action = ArgAction::Append)]
    pub settings_json: Vec<SettingJsonArg>,
    /// `--wait`, `--timeout`, and `--idempotency-key`.
    #[command(flatten)]
    pub run: AsyncArgs,
}

impl CreateArgs {
    /// The params of `instance.create` these flags ask for.
    #[must_use]
    pub fn params(&self) -> CreateInstanceParams {
        let mut settings = serde_json::Map::new();
        for setting in &self.settings {
            settings.insert(
                setting.field.clone(),
                serde_json::Value::String(setting.value.clone()),
            );
        }
        for setting in &self.settings_json {
            settings.insert(setting.field.clone(), setting.value.clone());
        }
        CreateInstanceParams {
            name: self.name.clone(),
            display_name: self.display_name.clone(),
            provider: ProviderSelector {
                name: self.provider.clone(),
                revision: Some(self.revision.clone()),
            },
            runtime_id: self.runtime,
            endpoints: self
                .ports
                .iter()
                .map(|port| EndpointRequest {
                    name: port.endpoint.clone(),
                    port: port.port,
                })
                .collect(),
            settings: SettingsDocument::new(settings),
            secrets: Vec::new(),
        }
    }
}

fn extract(reply: Reply) -> Option<Accepted> {
    match reply {
        Reply::InstanceCreate(reply) => Some(accepted(&reply)),
        _ => None,
    }
}

impl RunCommand for CreateArgs {
    type Output = InstanceAction;

    async fn run(self, cx: &mut Context) -> CommandOutput<InstanceAction> {
        let call = Call::InstanceCreate(self.params());
        match cx.submit(call, &self.run, extract).await {
            Ok(submission) => {
                let verb = Verb::Create { name: self.name };
                conclude(cx, submission, verb, None).await
            }
            Err(failure) => CommandOutput::failed(failure),
        }
    }
}
