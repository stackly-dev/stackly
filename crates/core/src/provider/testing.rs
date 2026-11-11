//! The facts the compatibility corpus freezes (spec 0036, *Corpus
//! kompatibilitas*): one function that `cargo xtask provider freeze` writes
//! `frozen.json` with and the `provider_compat` test checks against, so the
//! two can never disagree.
//!
//! Behind the `test-support` feature: the facts come from a fixed context
//! with fixture secrets, never from a real instance.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use stackly_domain::{InstanceId, InstanceName};
use stackly_platform::{DirectoryLayout, RootSet, SecretValue};

use super::plan::{InitStep, PlanContext, PlanError, ResolvedEndpoint, RuntimeContext};
use super::scenarios::{DEFAULT_SCENARIO, Scenario};
use super::validate::ValidatedPackage;
use crate::secret::ResolvedSecrets;

/// The `schema` of `frozen.json`.
pub const FROZEN_SCHEMA: &str = "stackly.provider-frozen/v1";
/// The instance ID of the frozen context.
const FROZEN_INSTANCE_ID: &str = "0192f3a6-0000-7000-8000-0000000000f0";
/// The instance name of the frozen context.
const FROZEN_INSTANCE_NAME: &str = "frozen";
/// The secret reference every secret field holds in the frozen settings;
/// its value is `frozen-<field>`.
const FROZEN_SECRET_REF: &str = "local:instance/0192f3a6-0000-7000-8000-0000000000f0/frozen";

/// What `frozen.json` records about one package revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[non_exhaustive]
pub struct FrozenFacts {
    /// Always [`FROZEN_SCHEMA`].
    pub schema: String,
    /// `metadata.id`.
    pub provider: String,
    /// `metadata.revision`.
    pub revision: String,
    /// The package digest, lowercase hex.
    pub manifest_sha256: String,
    /// The start command's arguments, without the program.
    pub start_args: Vec<String>,
    /// The start command's env keys, sorted.
    pub start_env_keys: Vec<String>,
    /// The SHA-256 of each rendered config file, by config file name.
    pub rendered: BTreeMap<String, String>,
}

/// Builds the frozen context for `package` and returns what its init and
/// run plans give there: the start argv and env keys, and the SHA-256 of
/// every rendered config file with the fixture secrets.
///
/// The context is fixed: instance `0192f3a6-0000-7000-8000-0000000000f0`
/// named `frozen`; folders from [`DirectoryLayout::instance_paths`] under
/// `/frozen/data`, `/frozen/cache`, `/frozen/logs`, and `/frozen/run`;
/// runtime root `/frozen/runtime` with the version and line of the first
/// artifact of `versions.json`; every tcp and udp endpoint on its
/// `defaultPort`; the settings of the scenario named `default` in
/// `scenarios` (empty without one) over the schema defaults; and every
/// secret field holding `frozen-<field>`.
///
/// # Errors
///
/// Returns the [`PlanError`] of the settings or of a plan that cannot be
/// built or rendered.
pub fn frozen_facts(
    package: &ValidatedPackage,
    scenarios: &[Scenario],
) -> Result<FrozenFacts, PlanError> {
    let (context, secrets) = frozen_context(package, scenarios)?;
    let init = package.init_plan(&context)?;
    let run = package.run_plan(&context)?;
    let mut rendered = BTreeMap::new();
    for step in init.steps.iter().chain(&run.render) {
        if let InitStep::WriteFile(write) = step {
            let bytes = write.content.materialize(&secrets)?;
            let digest: String = Sha256::digest(&bytes)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect();
            rendered.insert(write.content.config_file.clone(), digest);
        }
    }
    let mut start_env_keys: Vec<String> =
        run.spawn.env.iter().map(|(key, _)| key.clone()).collect();
    start_env_keys.sort();
    let key = package.key();
    Ok(FrozenFacts {
        schema: FROZEN_SCHEMA.to_owned(),
        provider: key.provider().to_string(),
        revision: key.revision().to_string(),
        manifest_sha256: key.manifest_sha256().to_string(),
        start_args: run.spawn.args.clone(),
        start_env_keys,
        rendered,
    })
}

fn frozen_context(
    package: &ValidatedPackage,
    scenarios: &[Scenario],
) -> Result<(PlanContext, ResolvedSecrets), PlanError> {
    let schema = package.settings_schema();
    let mut document = scenarios
        .iter()
        .find(|scenario| scenario.name.as_str() == DEFAULT_SCENARIO)
        .and_then(|scenario| scenario.settings.as_object().cloned())
        .unwrap_or_default();
    let mut secrets = Vec::new();
    for field in schema
        .fields()
        .filter(|field| field.is_secret() && field.managed().is_none())
    {
        document.insert(
            field.name().to_string(),
            serde_json::Value::String(FROZEN_SECRET_REF.to_owned()),
        );
        let value = SecretValue::new(format!("frozen-{}", field.name()))
            .expect("a frozen secret is UTF-8 without NUL");
        secrets.push((field.name().clone(), value));
    }
    let settings = schema
        .validate(&serde_json::Value::Object(document))
        .map_err(PlanError::Settings)?;
    let base = PathBuf::from("/frozen");
    let roots = RootSet::new(
        base.join("data"),
        base.join("logs"),
        base.join("cache"),
        base.join("run"),
    )
    .expect("the frozen roots are absolute and distinct");
    let id: InstanceId = FROZEN_INSTANCE_ID
        .parse()
        .expect("the frozen instance ID is a UUID v7");
    let (version, line) = package.versions().artifacts().first().map_or_else(
        || {
            (
                "0.0".parse().expect("0.0 is an exact version"),
                "0".parse().expect("0 is a runtime line"),
            )
        },
        |artifact| (artifact.version.clone(), artifact.line.clone()),
    );
    let endpoints = package
        .manifest()
        .endpoints
        .iter()
        .map(|(name, spec)| {
            let resolved = match spec.default_host {
                Some(host) => ResolvedEndpoint::Ip {
                    host,
                    port: spec.default_port,
                },
                None => ResolvedEndpoint::Unix,
            };
            (name.clone(), resolved)
        })
        .collect();
    let context = PlanContext {
        instance_id: id,
        instance_name: FROZEN_INSTANCE_NAME
            .parse::<InstanceName>()
            .expect("`frozen` is an instance name"),
        paths: DirectoryLayout::new(roots).instance_paths(id),
        runtime: RuntimeContext {
            root: base.join("runtime"),
            version,
            line,
        },
        endpoints,
        settings,
    };
    Ok((context, ResolvedSecrets::for_tests(secrets)))
}
