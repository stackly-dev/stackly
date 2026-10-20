//! Running the steps of a plan (spec 0010, *Executor plan*): the folders and
//! files every action writes, and the helper processes that init, stop, and
//! reload run.
//!
//! Every path that is written or removed passes
//! [`check_owned_path`](crate::check_owned_path) right before the IO, every
//! helper runs under the helper rule of [`run_helper`], and no secret ever
//! reaches an argument: [`ensure_not_in_argv`] runs before a spawn, and a
//! secret value reaches a child only through stdin, a file in `secrets/`,
//! or its explicit environment.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use stackly_platform::{ChildRole, FileKind, InstanceLock, SecretValue};

use super::SupervisorDeps;
use super::error::SupervisorError;
use super::helper::{HelperCommand, HelperEnd, run_helper};
use super::persist::blocking;
use super::prepare::Prepared;
use crate::provider::{Command, EnvValue, ExecutablePath, FileWrite, InitStep, RunSpec, Stdin};
use crate::secret::ResolvedSecrets;
use crate::secret::delivery::{SecretFile, ensure_not_in_argv, env_pair, stdin_payload};
use crate::{CoreError, check_owned_path, ensure_private_dir};

/// A step that could not run, as text, with the index of the step.
#[derive(Debug)]
pub(crate) struct StepFailure {
    /// The step's index in the plan, from 0.
    pub(crate) step: usize,
    /// Why, without any value of a secret.
    pub(crate) reason: String,
}

/// Makes each folder with mode 0700, parent first, accepting an existing one
/// only when it is a private folder of the user (spec 0009).
///
/// # Errors
///
/// Returns the [`CoreError`] of the first folder that cannot be made or
/// that exists and is not private.
pub(crate) fn ensure_directories(deps: &SupervisorDeps, dirs: &[PathBuf]) -> Result<(), CoreError> {
    let platform = &deps.platform;
    let user = platform.identity().current_user()?;
    for dir in dirs {
        let owned = check_owned_path(platform, &deps.layout, dir)?;
        let root = deps.layout.canonical_root(owned.root());
        let canonical = owned.canonical();
        let mut chain: Vec<&Path> = canonical
            .ancestors()
            .take_while(|ancestor| *ancestor != root)
            .collect();
        chain.reverse();
        for folder in chain {
            ensure_private_dir(platform.fs(), folder, &user)?;
        }
    }
    Ok(())
}

/// Writes a rendered config file atomically with mode 0600.
///
/// # Errors
///
/// Returns the [`CoreError`] of the path check or the write, or a
/// [`SupervisorError::ConfigInvalid`] text when the content cannot be
/// materialized.
pub(crate) fn write_config(
    deps: &SupervisorDeps,
    file: &FileWrite,
    bytes: &[u8],
) -> Result<(), CoreError> {
    check_owned_path(&deps.platform, &deps.layout, &file.path)?;
    deps.platform.fs().write_file_atomic(&file.path, bytes)?;
    Ok(())
}

/// Materializes the content of every `WriteFile` step before the first
/// side effect, so a template that cannot be written fails early. The
/// answer holds one entry per step, `Some` for a `WriteFile`.
pub(crate) fn materialize_all(
    steps: &[InitStep],
    secrets: &ResolvedSecrets,
) -> Result<Vec<Option<Vec<u8>>>, StepFailure> {
    steps
        .iter()
        .enumerate()
        .map(|(step, item)| match item {
            InitStep::WriteFile(file) => {
                file.content
                    .materialize(secrets)
                    .map(Some)
                    .map_err(|error| StepFailure {
                        step,
                        reason: error.to_string(),
                    })
            }
            _ => Ok(None),
        })
        .collect()
}

/// Runs the `EnsureDirectories` and `WriteFile` steps of a render, as every
/// spawn and every reload does first.
///
/// # Errors
///
/// Returns a [`SupervisorError::ConfigInvalid`] naming the step.
pub(crate) async fn run_render(
    deps: &SupervisorDeps,
    steps: &[InitStep],
    secrets: &ResolvedSecrets,
) -> Result<(), SupervisorError> {
    let invalid = |failure: StepFailure| SupervisorError::ConfigInvalid {
        field: format!("render step {}", failure.step),
        reason: failure.reason,
    };
    let contents = materialize_all(steps, secrets).map_err(invalid)?;
    for (index, (step, bytes)) in steps.iter().zip(contents).enumerate() {
        let deps = deps.clone();
        let step = step.clone();
        blocking(move || -> Result<(), StepFailure> {
            let fail = |error: CoreError| StepFailure {
                step: index,
                reason: error.to_string(),
            };
            match (&step, bytes) {
                (InitStep::EnsureDirectories(dirs), _) => {
                    ensure_directories(&deps, dirs).map_err(fail)
                }
                (InitStep::WriteFile(file), Some(bytes)) => {
                    write_config(&deps, file, &bytes).map_err(fail)
                }
                _ => Err(StepFailure {
                    step: index,
                    reason: "a render holds only EnsureDirectories and WriteFile steps".to_owned(),
                }),
            }
        })
        .await?
        .map_err(invalid)?;
    }
    Ok(())
}

/// Checks that `program` is a canonical regular file inside the runtime
/// folder that the OS can execute, and returns its canonical path.
///
/// # Errors
///
/// Returns [`SupervisorError::ExecutableInvalid`] naming what failed.
pub(crate) fn verify_executable(
    deps: &SupervisorDeps,
    prepared: &Prepared,
    program: &ExecutablePath,
) -> Result<PathBuf, SupervisorError> {
    let invalid = |reason: &str| SupervisorError::ExecutableInvalid {
        reason: reason.to_owned(),
    };
    let fs = deps.platform.fs();
    let canonical = fs
        .canonicalize(&program.absolute())
        .map_err(|_| invalid("the executable does not exist"))?;
    if !canonical.starts_with(&prepared.runtime_root) {
        return Err(invalid("the executable lies outside the runtime folder"));
    }
    let info = fs
        .symlink_metadata(&canonical)
        .map_err(|_| invalid("the executable cannot be read"))?
        .ok_or_else(|| invalid("the executable does not exist"))?;
    if info.kind != FileKind::File {
        return Err(invalid("the executable is not a regular file"));
    }
    if info.mode.is_some_and(|mode| mode & 0o111 == 0) {
        return Err(invalid("the executable is not executable"));
    }
    Ok(canonical)
}

/// A command as the platform takes it: the canonical program, the argv
/// after it, the environment, and the working folder.
pub(crate) struct CommandParts {
    pub(crate) program: PathBuf,
    pub(crate) args: Vec<String>,
    pub(crate) env: Vec<(String, String)>,
    pub(crate) cwd: PathBuf,
}

/// Resolves the program, checks the arguments for a secret, and builds the
/// environment, with a secret value only where the plan puts one.
///
/// # Errors
///
/// Returns [`SupervisorError::ExecutableInvalid`] for a program that fails
/// the checks, [`SupervisorError::SecretInArgv`] for an argument that
/// carries a secret, and [`SupervisorError::ConfigInvalid`] for an
/// environment value that cannot be resolved.
pub(crate) fn command_parts(
    deps: &SupervisorDeps,
    prepared: &Prepared,
    command: &Command,
    secrets: &ResolvedSecrets,
) -> Result<CommandParts, SupervisorError> {
    let program = verify_executable(deps, prepared, &command.program)?;
    ensure_not_in_argv(&command.args, secrets)?;
    let mut env = Vec::with_capacity(command.env.len());
    for (name, value) in &command.env {
        let text = match value {
            EnvValue::Text(text) => text.clone(),
            EnvValue::Secret(secret) => {
                let value =
                    secrets
                        .get(&secret.field)
                        .ok_or_else(|| SupervisorError::ConfigInvalid {
                            field: secret.field.to_string(),
                            reason: "the secret has no resolved value".to_owned(),
                        })?;
                env_pair(name, value)
                    .1
                    .to_str()
                    .ok_or_else(|| SupervisorError::ConfigInvalid {
                        field: secret.field.to_string(),
                        reason: "the secret is not text".to_owned(),
                    })?
                    .to_owned()
            }
        };
        env.push((name.clone(), text));
    }
    Ok(CommandParts {
        program,
        args: command.args.clone(),
        env,
        cwd: command.cwd.clone(),
    })
}

/// What a `Run` step's stdin becomes: a copy of a resolved secret, the
/// literal text, or nothing (spec 0019).
///
/// # Errors
///
/// Returns a [`StepFailure`] text when the secret has no value or the text
/// is not a valid stdin value.
pub(crate) fn stdin_value(
    stdin: &Stdin,
    secrets: &ResolvedSecrets,
) -> Result<Option<SecretValue>, String> {
    match stdin {
        Stdin::Null => Ok(None),
        Stdin::Secret(secret) => {
            let value = secrets
                .get(&secret.field)
                .ok_or_else(|| format!("the secret {} has no resolved value", secret.field))?;
            SecretValue::from_utf8(stdin_payload(value).to_vec())
                .map(Some)
                .map_err(|error| format!("the secret {} cannot be written: {error}", secret.field))
        }
        Stdin::Text(text) => SecretValue::new(text.clone())
            .map(Some)
            .map_err(|error| format!("the stdin text cannot be written: {error}")),
    }
}

/// Spawns one `Run` command as a helper and waits up to `limit`, applying
/// the helper rule when it runs past it.
///
/// # Errors
///
/// Returns the failure of the command as text: it could not be set up or
/// spawned.
pub(crate) async fn run_command(
    deps: &SupervisorDeps,
    prepared: &Prepared,
    spec: &RunSpec,
    secrets: &ResolvedSecrets,
    role: ChildRole,
    lock: Option<Arc<InstanceLock>>,
    limit: Duration,
) -> Result<HelperEnd, String> {
    let parts =
        command_parts(deps, prepared, &spec.command, secrets).map_err(|error| error.to_string())?;
    let stdin = stdin_value(&spec.stdin, secrets)?;
    let command = HelperCommand {
        program: parts.program,
        args: parts.args,
        env: parts.env,
        cwd: parts.cwd,
        role,
        stdin,
        lock,
    };
    run_helper(
        &deps.platform,
        command,
        limit,
        deps.config.helper_term_grace,
    )
    .await
    .map_err(|error| error.to_string())
}

/// Writes a one-time secret file for a `MaterializeSecret` step, which the
/// guard removes when it drops.
///
/// # Errors
///
/// Returns the failure as text.
pub(crate) fn materialize_secret(
    deps: &SupervisorDeps,
    prepared: &Prepared,
    file_name: &str,
    secret: &crate::provider::PlanSecret,
    secrets: &ResolvedSecrets,
) -> Result<SecretFile, String> {
    let value = secrets
        .get(&secret.field)
        .ok_or_else(|| format!("the secret {} has no resolved value", secret.field))?;
    SecretFile::create(&deps.platform, &deps.layout, prepared.key, file_name, value)
        .map_err(|error| error.to_string())
}
