//! A spike [`Recipe`] built from a provider package (spec 0036, *Spike dari
//! paket*, AC-16; spec 0003, *Spike dari paket provider*).
//!
//! Init, start, readiness, the observed version, and stop all come from the
//! package in `providers/` whose `metadata.software` is the inventory
//! provider, planned by `stackly-core` in a context the worker owns: the
//! run folder for data, cache, and logs, the private socket folder as the
//! runtime root, one allocated loopback port per TCP or UDP endpoint, the
//! settings of the package's `default` scenario, and a fixture credential
//! (spec 0002) for every secret field. The closure probes stay in
//! [`closure_probes`](super::closure_probes).
//!
//! This module is the one place in xtask that reads a secret value with
//! `expose`: it hands the fixture values it generated itself to the
//! initialize steps, on stdin, in an env value, or in a 0600 file.

use std::collections::BTreeMap;
use std::fs;
use std::net::{IpAddr, Ipv4Addr};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, anyhow, bail};
use stackly_core::provider::{
    Command as PlanCommand, EnvValue, InitStep, NameCheck, PlanContext, ProbeSpec,
    ResolvedEndpoint, RunSpec, RuntimeContext, Scenario, StopPlan, StopSignal, ValidatedPackage,
    check_package, manifest_software,
};
use stackly_core::secret::{Redactor, ResolvedSecrets};
use stackly_core::supervisor::SupervisorConfig;
use stackly_core::supervisor::probe::{self as core_probe, ProbeContext, TcpConnect};
use stackly_domain::{InstanceId, InstanceName, Port};
use stackly_evidence::FIXTURE_CREDENTIAL_PREFIX;
use stackly_platform::{DirectoryLayout, RootSet, SecretValue};
use stackly_platform_macos::{MacosFilesystem, MacosPlatform};
use uuid::Uuid;

use super::closure_probes::{self, EndpointFact, ProbeClient, SpikeFacts};
use super::process::{self, Finished, Server};
use super::providers::{CLIENT_TIMEOUT, Ctx, HTTP_TIMEOUT, Recipe};
use super::result::Probe;
use crate::runtime::ports;

/// The instance name of the spike context.
const SPIKE_INSTANCE_NAME: &str = "spike";
/// The keys of the emptied environment of AC-6 (spec 0003); the package's
/// env only adds keys to it.
const CLEAN_ENV_KEYS: [&str; 4] = ["PATH", "HOME", "TMPDIR", "LANG"];

/// The package folder in `<repo>/providers/` whose `metadata.software` is
/// `provider`: an `official/` one first, otherwise the first by folder
/// name. `None` when no package provides it.
///
/// # Errors
///
/// Fails when `providers/` cannot be listed.
pub fn find_package(repo: &Path, provider: &str) -> Result<Option<PathBuf>> {
    let dir = repo.join("providers");
    let mut folders: Vec<PathBuf> = fs::read_dir(&dir)
        .with_context(|| format!("cannot read {}", dir.display()))?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.is_dir())
        .collect();
    folders.sort();
    let mut found = Vec::new();
    for folder in folders {
        let Ok(text) = fs::read_to_string(folder.join("provider.yaml")) else {
            continue;
        };
        if manifest_software(&text).is_ok_and(|software| software.as_str() == provider) {
            let official = text.contains("id: official/");
            found.push((!official, folder));
        }
    }
    found.sort();
    Ok(found.into_iter().next().map(|(_, folder)| folder))
}

/// The lifecycle of one provider package.
pub struct PackageRecipe {
    /// The inventory provider, which picks the closure probes.
    provider: String,
    package: ValidatedPackage,
    /// The settings of the `default` scenario, over the schema defaults.
    settings: serde_json::Map<String, serde_json::Value>,
    instance_id: InstanceId,
    /// The fixture value of every secret field.
    secrets: BTreeMap<String, String>,
}

impl std::fmt::Debug for PackageRecipe {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PackageRecipe")
            .field("provider", &self.provider)
            .field("package", self.package.key())
            .finish_non_exhaustive()
    }
}

impl PackageRecipe {
    /// Checks the package in `dir` and builds its recipe.
    ///
    /// # Errors
    ///
    /// `package <folder> is not publishable` for a package with an error
    /// finding, and `package has no versionCheck` for one without
    /// `runtime.versionCheck`.
    pub fn load(dir: &Path, provider: &str) -> Result<Self> {
        let check = check_package(&MacosFilesystem, dir, NameCheck::FolderName);
        let Some(package) = check.package.clone().filter(|_| check.publishable()) else {
            bail!("package {} is not publishable", check.folder);
        };
        if package.version_check().is_none() {
            bail!("package has no versionCheck");
        }
        let settings = check
            .scenarios
            .iter()
            .find(|scenario| scenario.name.as_str() == "default")
            .unwrap_or(&Scenario::implied_default())
            .settings
            .as_object()
            .cloned()
            .unwrap_or_default();
        let secrets = package
            .settings_schema()
            .fields()
            .filter(|field| field.is_secret() && field.managed().is_none())
            .map(|field| {
                (
                    field.name().to_string(),
                    format!("{FIXTURE_CREDENTIAL_PREFIX}{}", Uuid::now_v7().simple()),
                )
            })
            .collect();
        Ok(Self {
            provider: provider.to_owned(),
            package,
            settings,
            instance_id: InstanceId::from_uuid(Uuid::now_v7())
                .map_err(|err| anyhow!("a new UUID v7 is an instance ID: {err}"))?,
            secrets,
        })
    }

    /// The TCP and UDP endpoints, in name order: one allocated port each.
    fn ip_endpoints(&self) -> usize {
        self.package
            .endpoints()
            .filter(|endpoint| endpoint.transport().is_ip())
            .count()
    }

    /// The fixture secrets for plans and the redactor.
    fn resolved_secrets(&self) -> Result<ResolvedSecrets> {
        let mut values = Vec::new();
        for field in self.package.settings_schema().fields() {
            if let Some(value) = self.secrets.get(field.name().as_str()) {
                let value = SecretValue::new(value.clone())
                    .map_err(|err| anyhow!("a fixture credential is a secret value: {err}"))?;
                values.push((field.name().clone(), value));
            }
        }
        Ok(ResolvedSecrets::for_tests(values))
    }

    /// The plan context of `ctx`, the same on every call.
    fn context(&self, ctx: &Ctx) -> Result<PlanContext> {
        let roots = RootSet::new(
            ctx.run_dir.join("data"),
            ctx.run_dir.join("logs"),
            ctx.run_dir.join("cache"),
            ctx.socket_dir()?.to_path_buf(),
        )
        .map_err(|err| anyhow!("the spike roots are not valid: {err}"))?;
        let mut endpoints = BTreeMap::new();
        let mut index = 0;
        for endpoint in self.package.endpoints() {
            let resolved = if endpoint.transport().is_ip() {
                let port = ctx.port(index)?;
                index += 1;
                ResolvedEndpoint::Ip {
                    host: endpoint
                        .default_host()
                        .unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST)),
                    port: Some(Port::new(port).map_err(|err| anyhow!("port {port}: {err}"))?),
                }
            } else {
                ResolvedEndpoint::Unix
            };
            endpoints.insert(endpoint.name().clone(), resolved);
        }
        let mut document = self.settings.clone();
        for name in self.secrets.keys() {
            document.insert(
                name.clone(),
                serde_json::Value::String(format!("local:instance/{}/spike", self.instance_id)),
            );
        }
        let settings = self
            .package
            .settings_schema()
            .validate(&serde_json::Value::Object(document))
            .map_err(|err| anyhow!("the default scenario settings do not validate: {err:?}"))?;
        Ok(PlanContext {
            instance_id: self.instance_id,
            instance_name: SPIKE_INSTANCE_NAME
                .parse::<InstanceName>()
                .map_err(|err| anyhow!("`spike` is an instance name: {err}"))?,
            paths: DirectoryLayout::new(roots).instance_paths(self.instance_id),
            runtime: RuntimeContext {
                root: ctx.artifact_root.clone(),
                version: ctx
                    .version
                    .parse()
                    .map_err(|err| anyhow!("version {}: {err}", ctx.version))?,
                line: ctx
                    .line
                    .parse()
                    .map_err(|err| anyhow!("line {}: {err}", ctx.line))?,
            },
            endpoints,
            settings,
        })
    }

    /// What the closure probes read about the instance.
    fn facts(&self, context: &PlanContext) -> SpikeFacts {
        let endpoints = context
            .endpoints
            .iter()
            .map(|(name, resolved)| {
                let fact = match resolved {
                    ResolvedEndpoint::Ip { port, .. } => EndpointFact {
                        port: port.map(Port::get),
                        socket_dir: None,
                    },
                    ResolvedEndpoint::Unix => EndpointFact {
                        port: None,
                        socket_dir: Some(context.paths.run.clone()),
                    },
                };
                (name.to_string(), fact)
            })
            .collect();
        SpikeFacts {
            endpoints,
            settings: context.settings.to_json(),
            secrets: self.secrets.clone(),
        }
    }

    /// The command of a plan step on top of the emptied environment: the
    /// plan's program, args, and working folder, and only the env keys the
    /// emptied environment does not set.
    fn command(ctx: &Ctx, plan: &PlanCommand, secrets: &ResolvedSecrets) -> Result<Command> {
        let mut command = ctx.env.command(&plan.program.absolute());
        command.args(&plan.args).current_dir(&plan.cwd);
        for (key, value) in &plan.env {
            if CLEAN_ENV_KEYS.contains(&key.as_str()) {
                continue;
            }
            match value {
                EnvValue::Text(text) => {
                    command.env(key, text);
                }
                EnvValue::Secret(secret) => {
                    let value = secrets
                        .get(&secret.field)
                        .with_context(|| format!("no fixture value for {}", secret.field))?;
                    command.env(key, value.expose());
                }
            }
        }
        Ok(command)
    }

    /// Runs one `runExecutable` or stop command and checks its exit code.
    fn run_spec(
        ctx: &mut Ctx,
        name: &str,
        spec: &RunSpec,
        secrets: &ResolvedSecrets,
    ) -> Result<()> {
        let command = Self::command(ctx, &spec.command, secrets)?;
        let input = match &spec.stdin {
            stackly_core::provider::Stdin::Null => None,
            stackly_core::provider::Stdin::Text(text) => Some(text.clone().into_bytes()),
            stackly_core::provider::Stdin::Secret(secret) => Some(
                secrets
                    .get(&secret.field)
                    .with_context(|| format!("no fixture value for {}", secret.field))?
                    .expose()
                    .as_bytes()
                    .to_vec(),
            ),
        };
        let finished = ctx.run_logged_input(name, command, input, spec.timeout)?;
        exit_matches(&finished, name, &spec.success_exit_codes)
    }

    /// Runs the render steps of a plan: folders and config files.
    fn write_steps(steps: &[InitStep], secrets: &ResolvedSecrets) -> Result<()> {
        for step in steps {
            match step {
                InitStep::EnsureDirectories(dirs) => {
                    for dir in dirs {
                        private_dir(dir)?;
                    }
                }
                InitStep::WriteFile(write) => {
                    let bytes = write
                        .content
                        .materialize(secrets)
                        .with_context(|| format!("cannot render {}", write.content.config_file))?;
                    private_file(&write.path, &bytes)?;
                }
                InitStep::MaterializeSecret { .. } | InitStep::Run(_) => {
                    bail!("a render step holds more than folders and files")
                }
            }
        }
        Ok(())
    }

    /// One readiness attempt through `stackly_core::supervisor::probe::run`.
    fn attempt(&self, ctx: &Ctx, probe: &ProbeSpec, timeout: std::time::Duration) -> Result<bool> {
        let roots = RootSet::new(
            ctx.run_dir.join("data"),
            ctx.run_dir.join("logs"),
            ctx.run_dir.join("cache"),
            ctx.socket_dir()?.to_path_buf(),
        )
        .map_err(|err| anyhow!("the spike roots are not valid: {err}"))?;
        let platform =
            MacosPlatform::with_roots(roots).map_err(|err| anyhow!("no macOS platform: {err}"))?;
        let redactor = Redactor::new();
        for value in self.secrets.values() {
            if let Ok(value) = SecretValue::new(value.clone()) {
                redactor.learn(&value);
            }
        }
        let config = SupervisorConfig::default();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .context("cannot start the probe runtime")?;
        let outcome = runtime.block_on(core_probe::run(
            probe,
            timeout,
            ProbeContext {
                platform: &platform,
                redactor: &redactor,
                config: &config,
                native: &TcpConnect,
            },
        ));
        Ok(outcome.is_ok())
    }
}

impl Recipe for PackageRecipe {
    /// Always: the short socket folder is the runtime root of the plan
    /// context, which holds the instance's run folder (its sockets and its
    /// `secrets` folder) within the 76 byte limit of the runtime root.
    fn uses_socket_dir(&self) -> bool {
        true
    }

    /// Allocates the ports, makes the instance folders, and runs the
    /// package's initialize plan, then removes its cleanup entries.
    fn init(&self, ctx: &mut Ctx) -> Result<()> {
        ctx.ports = ports::allocate(self.ip_endpoints())?;
        let context = self.context(ctx)?;
        let paths = &context.paths;
        for dir in [
            &paths.root,
            &paths.data,
            &paths.config,
            &paths.config_history,
            &paths.temp,
            &paths.logs,
            &paths.run,
        ] {
            private_dir(dir)?;
        }
        let secrets = self.resolved_secrets()?;
        let plan = self
            .package
            .init_plan(&context)
            .map_err(|err| anyhow!("cannot plan initialize: {err}"))?;
        let outcome = (|| {
            for (index, step) in plan.steps.iter().enumerate() {
                match step {
                    InitStep::EnsureDirectories(_) | InitStep::WriteFile(_) => {
                        Self::write_steps(std::slice::from_ref(step), &secrets)?;
                    }
                    InitStep::MaterializeSecret { secret, file_name } => {
                        let value = secrets
                            .get(&secret.field)
                            .with_context(|| format!("no fixture value for {}", secret.field))?;
                        let dir = paths.run.join("secrets");
                        private_dir(&dir)?;
                        private_file(&dir.join(file_name), value.expose().as_bytes())?;
                    }
                    InitStep::Run(spec) => {
                        Self::run_spec(ctx, &format!("initialize[{index}]"), spec, &secrets)?;
                    }
                }
            }
            Ok(())
        })();
        for path in &plan.cleanup {
            // Ignored: an entry that was never written is already gone.
            let _ = if path.is_dir() {
                fs::remove_dir_all(path)
            } else {
                fs::remove_file(path)
            };
        }
        outcome
    }

    /// Renders every config file, then returns the start command.
    fn start(&self, ctx: &mut Ctx) -> Result<Command> {
        let context = self.context(ctx)?;
        let secrets = self.resolved_secrets()?;
        let plan = self
            .package
            .run_plan(&context)
            .map_err(|err| anyhow!("cannot plan start: {err}"))?;
        Self::write_steps(&plan.render, &secrets)?;
        Self::command(ctx, &plan.spawn, &secrets)
    }

    fn ready(&self, ctx: &Ctx) -> Result<bool> {
        let context = self.context(ctx)?;
        let plan = self
            .package
            .run_plan(&context)
            .map_err(|err| anyhow!("cannot plan start: {err}"))?;
        self.attempt(ctx, &plan.readiness.probe, plan.readiness.attempt_timeout)
    }

    /// The main executable with the `versionCheck` args: stdout, then
    /// stderr.
    fn version(&self, ctx: &Ctx) -> Result<String> {
        let check = self
            .package
            .version_check()
            .context("package has no versionCheck")?;
        let context = self.context(ctx)?;
        let plan = self
            .package
            .run_plan(&context)
            .map_err(|err| anyhow!("cannot plan start: {err}"))?;
        let mut command = ctx.env.command(&plan.spawn.program.absolute());
        command.args(check.args());
        let finished = process::run(command, check.timeout())?;
        if finished.status.is_none() {
            bail!(
                "the version check did not finish within {} s",
                check.timeout().as_secs()
            );
        }
        Ok(format!("{}\n{}", finished.stdout, finished.stderr))
    }

    /// The capture group of the `versionCheck` pattern.
    fn normalize(&self, raw: &str) -> Option<String> {
        self.package
            .version_check()?
            .read_version(raw, "")
            .map(str::to_owned)
    }

    fn probes(&self, ctx: &Ctx) -> Vec<Probe> {
        match self.context(ctx) {
            Ok(context) => {
                closure_probes::probes(&self.provider, &self.facts(&context), &CtxClient { ctx })
            }
            Err(_) => Vec::new(),
        }
    }

    fn stop(&self, ctx: &Ctx, server: &Server) -> Result<()> {
        let context = self.context(ctx)?;
        let plan = self
            .package
            .run_plan(&context)
            .map_err(|err| anyhow!("cannot plan start: {err}"))?;
        match &plan.stop {
            StopPlan::Signal { signal, .. } => server.signal(match signal {
                StopSignal::Term => "TERM",
                StopSignal::Int => "INT",
                StopSignal::Quit => "QUIT",
                other => bail!("the spike cannot send stop signal {other:?}"),
            }),
            StopPlan::Command { run, .. } => {
                let secrets = self.resolved_secrets()?;
                let mut ctx = Ctx {
                    logs: Vec::new(),
                    ..ctx.clone()
                };
                Self::run_spec(&mut ctx, "stop", run, &secrets)
            }
        }
    }

    fn version_args(&self) -> Vec<String> {
        self.package
            .version_check()
            .map(|check| check.args().to_vec())
            .unwrap_or_default()
    }

    fn log_files(&self, ctx: &Ctx) -> Vec<(String, PathBuf)> {
        let Ok(context) = self.context(ctx) else {
            return Vec::new();
        };
        self.package
            .run_plan(&context)
            .map(|plan| {
                plan.log_files
                    .into_iter()
                    .map(|path| {
                        let name = path
                            .file_name()
                            .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
                        (name, path)
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// The closure probes' way to the server: the inventory executables in
/// the emptied environment, plus the probe's own env.
struct CtxClient<'a> {
    ctx: &'a Ctx,
}

impl ProbeClient for CtxClient<'_> {
    fn run(&self, name: &str, args: &[String], env: &[(String, String)]) -> Result<Finished> {
        let mut command = self.ctx.command(name)?;
        command.args(args);
        for (key, value) in env {
            command.env(key, value);
        }
        process::run(command, CLIENT_TIMEOUT)
    }

    fn http_get(&self, port: u16, path: &str) -> Result<(u16, String)> {
        process::http_get(port, path, HTTP_TIMEOUT)
    }

    fn file_len(&self, path: &Path) -> Option<u64> {
        fs::metadata(path).ok().map(|meta| meta.len())
    }

    fn run_dir(&self) -> &Path {
        &self.ctx.run_dir
    }
}

/// Fails unless the command ended with one of `codes`.
fn exit_matches(finished: &Finished, what: &str, codes: &[u8]) -> Result<()> {
    match finished.status {
        Some(status) => match status.code() {
            Some(code) if u8::try_from(code).is_ok_and(|code| codes.contains(&code)) => Ok(()),
            _ => bail!(
                "{what} failed with {}: {}",
                process::describe(status),
                finished.complaint()
            ),
        },
        None => bail!("{what} did not finish in time"),
    }
}

/// A folder and its parents, the folder itself with mode 0700.
fn private_dir(dir: &Path) -> Result<()> {
    fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
    fs::set_permissions(dir, fs::Permissions::from_mode(0o700))
        .with_context(|| format!("cannot restrict {}", dir.display()))
}

/// A file with mode 0600, its folder made first.
fn private_file(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("cannot create {}", parent.display()))?;
    }
    fs::write(path, bytes).with_context(|| format!("cannot write {}", path.display()))?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .with_context(|| format!("cannot restrict {}", path.display()))
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::super::process::CleanEnv;
    use super::*;

    fn repo() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
    }

    /// A short socket folder in `/tmp`, removed on drop, as the worker
    /// makes it.
    struct SocketDir(PathBuf);

    impl SocketDir {
        fn new() -> Self {
            let dir = PathBuf::from(format!(
                "{}{}",
                super::super::SOCKET_PREFIX,
                &Uuid::now_v7().simple().to_string()[24..]
            ));
            private_dir(&dir).unwrap();
            Self(dir)
        }
    }

    impl Drop for SocketDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// A worker context in `root`, as `Spike::init` makes it.
    fn ctx(
        root: &Path,
        artifact_root: &Path,
        version: &str,
        line: &str,
        socket: &SocketDir,
    ) -> Ctx {
        let run_dir = root.join("run");
        let env = CleanEnv {
            cwd: run_dir.clone(),
            home: run_dir.join("home"),
            tmpdir: run_dir.join("tmp"),
        };
        for dir in [&run_dir, &env.home, &env.tmpdir] {
            fs::create_dir_all(dir).unwrap();
        }
        let socket_dir = Some(socket.0.clone());
        Ctx {
            artifact_root: artifact_root.to_path_buf(),
            version: version.to_owned(),
            line: line.to_owned(),
            executables: Vec::new(),
            data_dir: run_dir.join("data"),
            run_dir,
            env,
            socket_dir,
            ports: Vec::new(),
            logs: Vec::new(),
        }
    }

    fn load(provider: &str) -> PackageRecipe {
        let dir = find_package(&repo(), provider).unwrap().unwrap();
        PackageRecipe::load(&dir, provider).unwrap()
    }

    // covers: AC-16 (spec 0036)
    #[test]
    fn the_packaged_providers_are_found_by_their_software() {
        for provider in ["nats", "postgresql", "traefik", "valkey"] {
            let dir = find_package(&repo(), provider).unwrap().unwrap();
            assert_eq!(dir.file_name().unwrap().to_string_lossy(), provider);
        }
        for provider in ["mysql", "clickhouse", "redis"] {
            assert_eq!(find_package(&repo(), provider).unwrap(), None, "{provider}");
        }
    }

    // covers: AC-16 (spec 0036)
    #[test]
    fn a_package_that_is_not_publishable_or_has_no_version_check_is_refused() {
        let root = stackly_testkit::TestRoot::new().unwrap();
        let broken = root.path().join("fake-command");
        copy_dir(
            &repo().join("crates/core/tests/fixtures/packages/fake-command"),
            &broken,
        );
        let err = PackageRecipe::load(&broken, "fake").unwrap_err();
        assert_eq!(err.to_string(), "package fake-command is not publishable");

        let nats = root.path().join("nats");
        copy_dir(&repo().join("providers/nats"), &nats);
        let manifest = nats.join("provider.yaml");
        let text = fs::read_to_string(&manifest).unwrap();
        let start = text.find("  versionCheck:").unwrap();
        let end = text[start..].find("capabilities:").unwrap() + start;
        fs::write(&manifest, format!("{}{}", &text[..start], &text[end..])).unwrap();
        let err = PackageRecipe::load(&nats, "nats").unwrap_err();
        assert_eq!(err.to_string(), "package has no versionCheck");
    }

    // covers: AC-16 (spec 0036)
    #[test]
    fn nats_starts_from_its_package_with_the_allocated_ports() {
        let root = stackly_testkit::TestRoot::new().unwrap();
        let recipe = load("nats");
        let socket = SocketDir::new();
        let mut ctx = ctx(
            root.path(),
            &root.path().join("art"),
            "2.15.1",
            "2.15",
            &socket,
        );
        ctx.ports = ports::allocate(recipe.ip_endpoints()).unwrap();

        let command = recipe.start(&mut ctx).unwrap();

        assert!(recipe.uses_socket_dir());
        assert_eq!(recipe.ip_endpoints(), 2);
        assert_eq!(
            command.get_program(),
            root.path().join("art/nats-server").as_os_str()
        );
        let args: Vec<String> = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert_eq!(args[0], "--config");
        let config = fs::read_to_string(&args[1]).unwrap();
        for port in &ctx.ports {
            assert!(config.contains(&port.to_string()), "{config}");
        }
        assert_eq!(recipe.version_args(), ["--version"]);
        assert_eq!(
            recipe.normalize("nats-server: v2.15.1\n").as_deref(),
            Some("2.15.1")
        );
        let env: Vec<String> = command
            .get_envs()
            .map(|(key, _)| key.to_string_lossy().into_owned())
            .collect();
        for key in CLEAN_ENV_KEYS {
            assert!(env.contains(&key.to_owned()), "{env:?}");
        }
    }

    // covers: AC-16 (spec 0036)
    #[test]
    fn postgresql_17_and_18_plan_initdb_with_the_fixture_password_and_a_socket() {
        for (version, line) in [("17.11", "17"), ("18.6", "18")] {
            let root = stackly_testkit::TestRoot::new().unwrap();
            let recipe = load("postgresql");
            let socket = SocketDir::new();
            let mut ctx = ctx(
                root.path(),
                &root.path().join("art"),
                version,
                line,
                &socket,
            );
            ctx.ports = ports::allocate(recipe.ip_endpoints()).unwrap();
            let context = recipe.context(&ctx).unwrap();

            let init = recipe.package.init_plan(&context).unwrap();
            let command = recipe.start(&mut ctx).unwrap();

            assert!(recipe.uses_socket_dir());
            let initdb = init
                .steps
                .iter()
                .find_map(|step| match step {
                    InitStep::Run(spec) => Some(spec),
                    _ => None,
                })
                .unwrap();
            assert!(initdb.command.program.absolute().ends_with("bin/initdb"));
            assert!(initdb.command.args.contains(&"-Upostgres".to_owned()));
            assert!(
                init.steps
                    .iter()
                    .any(|step| matches!(step, InitStep::MaterializeSecret { .. }))
            );
            assert!(
                command
                    .get_program()
                    .to_string_lossy()
                    .ends_with("bin/postgres")
            );
            let password = &recipe.secrets["superuserPassword"];
            assert!(password.starts_with(FIXTURE_CREDENTIAL_PREFIX));
            let facts = recipe.facts(&context);
            assert_eq!(facts.endpoints["client"].port, Some(ctx.ports[0]));
            assert_eq!(
                facts.endpoints["socket"].socket_dir.as_deref(),
                Some(context.paths.run.as_path())
            );
            assert!(
                context
                    .paths
                    .run
                    .starts_with(ctx.socket_dir.as_ref().unwrap())
            );
            assert_eq!(recipe.version_args(), ["--version"]);
            assert_eq!(
                recipe
                    .normalize(&format!("postgres (PostgreSQL) {version}\n"))
                    .as_deref(),
                Some(version)
            );
        }
    }

    // covers: AC-16 (spec 0036)
    #[test]
    fn traefik_starts_from_its_package_and_reads_its_version_line() {
        let root = stackly_testkit::TestRoot::new().unwrap();
        let recipe = load("traefik");
        let socket = SocketDir::new();
        let mut ctx = ctx(
            root.path(),
            &root.path().join("art"),
            "3.7.15",
            "3.7",
            &socket,
        );
        ctx.ports = ports::allocate(recipe.ip_endpoints()).unwrap();

        let command = recipe.start(&mut ctx).unwrap();

        assert_eq!(recipe.ip_endpoints(), 2);
        let args: Vec<String> = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        let config = args[0].strip_prefix("--configFile=").unwrap();
        assert!(Path::new(config).is_file());
        assert_eq!(recipe.version_args(), ["version"]);
        assert_eq!(
            recipe
                .normalize("Version:      3.7.15\nCodename:     x\n")
                .as_deref(),
            Some("3.7.15")
        );
    }

    // covers: AC-16 (spec 0040)
    #[test]
    fn valkey_starts_from_its_package_with_the_users_file_and_reads_its_version_line() {
        let root = stackly_testkit::TestRoot::new().unwrap();
        let recipe = load("valkey");
        let socket = SocketDir::new();
        let mut ctx = ctx(
            root.path(),
            &root.path().join("art"),
            "9.1.2",
            "9.1",
            &socket,
        );
        ctx.ports = ports::allocate(recipe.ip_endpoints()).unwrap();

        let command = recipe.start(&mut ctx).unwrap();

        assert_eq!(recipe.ip_endpoints(), 1);
        assert!(
            command
                .get_program()
                .to_string_lossy()
                .ends_with("bin/valkey-server")
        );
        let args: Vec<String> = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        let port = args.iter().position(|arg| arg == "--port").unwrap();
        assert_eq!(args[port + 1], ctx.ports[0].to_string());
        let users = args
            .iter()
            .find(|arg| arg.ends_with("/secrets/users.conf"))
            .unwrap();
        let text = fs::read_to_string(users).unwrap();
        let password = &recipe.secrets["password"];
        assert!(password.starts_with(FIXTURE_CREDENTIAL_PREFIX));
        assert!(!text.contains(password.as_str()), "{text}");
        assert!(text.contains("user stackly-readiness on nopass -@all +ping"));
        assert_eq!(recipe.version_args(), ["--version"]);
        assert_eq!(
            recipe
                .normalize("Valkey server v=9.1.2 sha=00000000:1 malloc=libc bits=64 build=991715b0e5b85611\n")
                .as_deref(),
            Some("9.1.2")
        );
        assert_eq!(recipe.normalize("valkey-cli 9.1.2\n"), None);
    }

    // covers: AC-16 (spec 0036)
    #[test]
    fn the_fake_package_runs_init_start_ready_and_stop_against_the_fake_server() {
        let root = stackly_testkit::TestRoot::new().unwrap();
        let artifact = root.path().join("art");
        fs::create_dir_all(&artifact).unwrap();
        fs::copy(
            stackly_testkit::fake_server::binary(),
            artifact.join("stackly-fake-server"),
        )
        .unwrap();
        let recipe = PackageRecipe::load(&repo().join("providers/fake"), "fake").unwrap();
        let first = recipe.package.versions().artifacts()[0].clone();
        let socket = SocketDir::new();
        let mut ctx = ctx(
            root.path(),
            &artifact,
            &first.version.to_string(),
            &first.line.to_string(),
            &socket,
        );

        recipe.init(&mut ctx).unwrap();
        let command = recipe.start(&mut ctx).unwrap();
        let mut server = Server::spawn("server", command, &ctx.run_dir).unwrap();
        let started = Instant::now();
        let ready = loop {
            if recipe.ready(&ctx).unwrap() {
                break true;
            }
            if started.elapsed() > Duration::from_secs(30) {
                break false;
            }
            std::thread::sleep(Duration::from_millis(100));
        };
        let raw = recipe.version(&ctx).unwrap();
        recipe.stop(&ctx, &server).unwrap();
        let exited = server.wait_for(Duration::from_secs(15)).unwrap();

        assert!(ready, "the fake server became ready");
        assert!(recipe.normalize(&raw).is_some(), "{raw}");
        assert!(exited.is_some(), "the fake server stopped on SIGTERM");
        assert!(
            ctx.logs
                .iter()
                .any(|log| log.process.starts_with("initialize["))
        );
        assert!(recipe.probes(&ctx).is_empty());
        let context = recipe.context(&ctx).unwrap();
        assert!(context.paths.run.starts_with(&socket.0));
    }

    fn copy_dir(from: &Path, to: &Path) {
        fs::create_dir_all(to).unwrap();
        for entry in fs::read_dir(from).unwrap() {
            let entry = entry.unwrap();
            let target = to.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                copy_dir(&entry.path(), &target);
            } else {
                fs::copy(entry.path(), &target).unwrap();
            }
        }
    }
}
