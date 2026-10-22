//! `execute_init` (spec 0010, *Executor plan*, Init): runs the `InitPlan` of
//! a provider for a new instance under the instance lock, then records
//! `init_succeeded` and the manifest's `data.initializedAt`, or
//! `init_failed` with the reason.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use stackly_domain::{FailureCode, InstanceId, InstanceLifecycle, LifecycleEvent};
use stackly_platform::{CapturedOutput, ChildRole, ExitStatus, InstanceLock, OutputStream};
use tokio::time::Instant;

use super::deps::SupervisorDeps;
use super::error::SupervisorError;
use super::helper::HelperEnd;
use super::lock::take_lock;
use super::persist::{CommitMode, apply_event, blocking, commit_instance};
use super::prepare::{Checks, Prepared, prepare};
use super::sink::{LogBatch, LogLine, SinkOutcome};
use super::spool::char_cut;
use super::steps::{
    ensure_directories, materialize_all, materialize_secret, run_command, write_config,
};
use super::{Supervisor, secrets};
use crate::provider::{InitStep, PlanError, RunSpec};
use crate::secret::delivery::{SecretFile, ensure_not_in_argv};
use crate::secret::{Redactor, ResolvedSecrets, SecretCaller};
use crate::{read_instance_manifest, remove_owned_tree, write_instance_manifest};

/// What [`Supervisor::execute_init`] needs.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct InitRequest {
    /// The instance to initialize: `uninitialized`, or `initializing`.
    pub instance: InstanceId,
    /// Who asks, which decides whether the secret store may show a dialog.
    pub caller: SecretCaller,
}

impl InitRequest {
    /// Asks to initialize `instance` for `caller`.
    #[must_use]
    pub const fn new(instance: InstanceId, caller: SecretCaller) -> Self {
        Self { instance, caller }
    }
}

/// How the steps of a plan ended, short of success.
#[derive(Debug)]
enum InitFault {
    Step {
        step: usize,
        exit_code: Option<i32>,
        reason: String,
    },
    StepTimeout {
        step: usize,
    },
    Deadline,
}

impl InitFault {
    fn code(&self) -> FailureCode {
        match self {
            Self::Step { .. } => FailureCode::INIT_STEP_FAILED,
            Self::StepTimeout { .. } => FailureCode::INIT_STEP_TIMEOUT,
            Self::Deadline => FailureCode::INIT_TIMEOUT,
        }
    }

    fn error(&self) -> SupervisorError {
        let (step, exit_code) = match self {
            Self::Step {
                step, exit_code, ..
            } => (Some(*step), *exit_code),
            Self::StepTimeout { step } => (Some(*step), None),
            Self::Deadline => (None, None),
        };
        SupervisorError::InitFailed {
            reason: self.code().as_str().to_owned(),
            step,
            exit_code,
        }
    }
}

impl Supervisor {
    /// Initializes the data of a new instance with the provider's
    /// `InitPlan`: takes the instance lock, resolves every secret before the
    /// first side effect, runs the steps under the plan's deadline, removes
    /// the secret files and the plan's cleanup paths whether it worked or
    /// not, and records the result.
    ///
    /// The lock is released before this returns an error, so a failed init
    /// never makes the compensation of the create operation refuse with
    /// `instance_locked` (spec 0010 K10).
    ///
    /// # Errors
    ///
    /// Returns `supervisor.data_in_use` when the lock is held, the secret
    /// errors (`secret.needs_secret` among them, which changes no
    /// lifecycle), `supervisor.config_invalid` when the plan cannot be
    /// built, and `supervisor.init_failed` when a step failed or timed out;
    /// in that case the lifecycle is `failed` with the reason.
    pub async fn execute_init(&self, request: InitRequest) -> Result<(), SupervisorError> {
        let deps = self.deps();
        let prepared = prepare(self, request.instance, Checks::Full).await?;
        match prepared.instance.lifecycle() {
            InstanceLifecycle::Uninitialized | InstanceLifecycle::Initializing => {}
            other => {
                return Err(SupervisorError::internal(format!(
                    "init needs an uninitialized instance, not {}",
                    other.as_code()
                )));
            }
        }
        let lock = take_lock(deps, &prepared).await?;
        self.init_locked(&prepared, &request, lock).await
    }

    /// The part of init that holds the lock; the lock is dropped when this
    /// returns, whatever the result.
    async fn init_locked(
        &self,
        prepared: &Prepared,
        request: &InitRequest,
        lock: Arc<InstanceLock>,
    ) -> Result<(), SupervisorError> {
        let deps = self.deps();
        let id = request.instance;
        let secrets = secrets::resolve(self, id, &request.caller).await?;
        let ctx = prepared.context(&std::collections::BTreeMap::new());
        let plan = prepared
            .package
            .init_plan(&ctx)
            .map_err(|error| config_invalid(&error))?;
        // Every side effect comes after these checks.
        for step in &plan.steps {
            if let InitStep::Run(spec) = step {
                ensure_not_in_argv(&spec.command.args, &secrets)?;
            }
        }
        let contents = materialize_all(&plan.steps, &secrets).map_err(|failure| {
            SupervisorError::ConfigInvalid {
                field: format!("init step {}", failure.step),
                reason: failure.reason,
            }
        })?;

        if prepared.instance.lifecycle() == InstanceLifecycle::Uninitialized {
            let now = deps.platform.clock().now();
            commit_instance(self, id, None, CommitMode::Operation, |changes| {
                apply_event(changes, LifecycleEvent::InitBegin, now)?;
                Ok(Vec::new())
            })
            .await?;
        }

        let outcome = self
            .run_init_steps(
                prepared,
                &plan.steps,
                contents,
                plan.deadline,
                &secrets,
                &lock,
            )
            .await;
        let cleanup = remove_cleanup(self, &plan.cleanup).await;
        if let Err(error) = cleanup {
            tracing::warn!(%error, "an init cleanup path could not be removed");
        }

        let now = deps.platform.clock().now();
        match outcome {
            Ok(()) => {
                commit_instance(self, id, None, CommitMode::Operation, |changes| {
                    apply_event(changes, LifecycleEvent::InitSucceeded, now)?;
                    Ok(Vec::new())
                })
                .await?;
                record_initialized(self, prepared, now).await
            }
            Err(fault) => {
                if let InitFault::Step { step, reason, .. } = &fault {
                    tracing::warn!(instance = %id, step, %reason, "an init step failed");
                }
                let reason = fault.code();
                commit_instance(self, id, None, CommitMode::Operation, |changes| {
                    apply_event(
                        changes,
                        LifecycleEvent::InitFailed {
                            reason: reason.clone(),
                        },
                        now,
                    )?;
                    Ok(Vec::new())
                })
                .await?;
                Err(fault.error())
            }
        }
    }

    /// Runs the steps in order under one deadline, keeping every secret
    /// file guard until the last step ended.
    async fn run_init_steps(
        &self,
        prepared: &Prepared,
        steps: &[InitStep],
        contents: Vec<Option<Vec<u8>>>,
        deadline: Duration,
        secrets: &ResolvedSecrets,
        lock: &Arc<InstanceLock>,
    ) -> Result<(), InitFault> {
        let deps = self.deps();
        let started = Instant::now();
        let mut guards: Vec<SecretFile> = Vec::new();
        for (index, (step, bytes)) in steps.iter().zip(contents).enumerate() {
            let remaining = deadline.saturating_sub(started.elapsed());
            if remaining.is_zero() {
                return Err(InitFault::Deadline);
            }
            let failed = |reason: String| InitFault::Step {
                step: index,
                exit_code: None,
                reason,
            };
            match step {
                InitStep::EnsureDirectories(dirs) => {
                    let deps = deps.clone();
                    let dirs = dirs.clone();
                    blocking(move || ensure_directories(&deps, &dirs))
                        .await
                        .map_err(|error| failed(error.to_string()))?
                        .map_err(|error| failed(error.to_string()))?;
                }
                InitStep::WriteFile(file) => {
                    let deps = deps.clone();
                    let file = file.clone();
                    let bytes =
                        bytes.ok_or_else(|| failed("the file has no content".to_owned()))?;
                    blocking(move || write_config(&deps, &file, &bytes))
                        .await
                        .map_err(|error| failed(error.to_string()))?
                        .map_err(|error| failed(error.to_string()))?;
                }
                InitStep::MaterializeSecret { secret, file_name } => {
                    let guard = materialize_secret(deps, prepared, file_name, secret, secrets)
                        .map_err(failed)?;
                    guards.push(guard);
                }
                InitStep::Run(spec) => {
                    self.run_init_command(prepared, spec, secrets, lock, index, remaining)
                        .await?;
                }
            }
        }
        drop(guards);
        Ok(())
    }

    /// Runs one `Run` step: the lock at fd 3, stdin from the plan, and the
    /// shorter of the step's timeout and the time left of the deadline.
    async fn run_init_command(
        &self,
        prepared: &Prepared,
        spec: &RunSpec,
        secrets: &ResolvedSecrets,
        lock: &Arc<InstanceLock>,
        index: usize,
        remaining: Duration,
    ) -> Result<(), InitFault> {
        let deps = self.deps();
        let limit = spec.timeout.min(remaining);
        let ended = run_command(
            deps,
            prepared,
            spec,
            secrets,
            ChildRole::InitStep,
            Some(Arc::clone(lock)),
            limit,
        )
        .await
        .map_err(|reason| InitFault::Step {
            step: index,
            exit_code: None,
            reason,
        })?;
        match ended {
            HelperEnd::TimedOut if remaining < spec.timeout => Err(InitFault::Deadline),
            HelperEnd::TimedOut => Err(InitFault::StepTimeout { step: index }),
            HelperEnd::Exited(notice) => {
                let fault = match notice.status {
                    Some(ExitStatus::Exited(code))
                        if u8::try_from(code)
                            .is_ok_and(|code| spec.success_exit_codes.contains(&code)) =>
                    {
                        return Ok(());
                    }
                    Some(ExitStatus::Exited(code)) => InitFault::Step {
                        step: index,
                        exit_code: Some(code),
                        reason: format!("the step exited with code {code}"),
                    },
                    other => InitFault::Step {
                        step: index,
                        exit_code: None,
                        reason: format!("the step ended without an exit code: {other:?}"),
                    },
                };
                if let Some(captured) = &notice.captured {
                    deliver_output(deps, prepared.instance.id(), captured);
                }
                Err(fault)
            }
        }
    }
}

/// Hands the output a failed step left in its exit notice to the log
/// sinks, so the failure excerpt of the instance shows it (spec 0010: the
/// output of a helper never enters a spool; core redacts it and uses it
/// for the failure).
fn deliver_output(deps: &SupervisorDeps, id: InstanceId, captured: &CapturedOutput) {
    for stream in OutputStream::ALL {
        let bytes = match stream {
            OutputStream::Stdout => &captured.stdout,
            OutputStream::Stderr => &captured.stderr,
        };
        let lines = output_lines(
            &deps.redactor,
            bytes,
            deps.config.helper_capture_max,
            deps.config.log_line_max,
        );
        if lines.is_empty() {
            continue;
        }
        let batch = LogBatch::new(stream, deps.platform.clock().now(), lines);
        if deps.sinks.accept(id, batch) == SinkOutcome::Busy {
            tracing::warn!(
                instance = %id,
                stream = stream.as_code(),
                "a log sink was busy, so the output of the failed init step did not reach it"
            );
        }
    }
}

/// Cuts the captured tail of one stream into lines by the rules of a
/// spool: lossy UTF-8, one line per newline without its `\r`, each whole
/// line redacted before it is cut into pieces of at most `line_max` bytes,
/// and a last line without a newline marked `partial`. A tail as long as
/// `capture_max` may start inside a line, and inside a secret value the
/// redactor would then miss, so that first line is dropped.
fn output_lines(
    redactor: &Redactor,
    bytes: &[u8],
    capture_max: usize,
    line_max: usize,
) -> Vec<LogLine> {
    let mut rest = bytes;
    if bytes.len() >= capture_max {
        rest = bytes
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(&[][..], |at| &bytes[at + 1..]);
    }
    let mut lines = Vec::new();
    while !rest.is_empty() {
        let (line, ended) = match rest.iter().position(|byte| *byte == b'\n') {
            Some(at) => {
                let line = &rest[..at];
                rest = &rest[at + 1..];
                (line, true)
            }
            None => (std::mem::take(&mut rest), false),
        };
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        let redacted = redactor.redact(&String::from_utf8_lossy(line)).into_owned();
        let mut piece_rest = redacted.as_str();
        let mut continued = false;
        loop {
            let (piece, tail) = piece_rest.split_at(char_cut(piece_rest, line_max));
            lines.push(LogLine {
                text: piece.to_owned(),
                partial: !ended && tail.is_empty(),
                continued,
            });
            if tail.is_empty() {
                break;
            }
            continued = true;
            piece_rest = tail;
        }
    }
    lines
}

/// Maps a [`PlanError`] to the dry run error of the start and init
/// transactions.
pub(crate) fn config_invalid(error: &PlanError) -> SupervisorError {
    match error {
        PlanError::Unsupported { capability } => SupervisorError::CapabilityUnsupported {
            capability: capability.to_string(),
        },
        other => SupervisorError::ConfigInvalid {
            field: String::new(),
            reason: other.to_string(),
        },
    }
}

/// Removes every cleanup path of the plan through
/// [`remove_owned_tree`](crate::remove_owned_tree), the only deleting path.
async fn remove_cleanup(sup: &Supervisor, paths: &[PathBuf]) -> Result<(), SupervisorError> {
    let deps = sup.deps().clone();
    let paths = paths.to_vec();
    blocking(move || {
        let mut first = None;
        for path in &paths {
            if let Err(error) = remove_owned_tree(&deps.platform, &deps.layout, path)
                && first.is_none()
            {
                first = Some(SupervisorError::internal(error.to_string()));
            }
        }
        first.map_or(Ok(()), Err)
    })
    .await?
}

/// Rewrites `instance-manifest.json` with `data.initializedAt`, the same
/// time the lifecycle event recorded.
async fn record_initialized(
    sup: &Supervisor,
    prepared: &Prepared,
    at: jiff::Timestamp,
) -> Result<(), SupervisorError> {
    let deps = sup.deps().clone();
    let paths = prepared.paths.clone();
    blocking(move || -> Result<(), crate::CoreError> {
        let mut manifest = read_instance_manifest(&deps.platform, &paths)?;
        manifest.record_initialized(at);
        manifest.record_write(at, crate::daemon_info().version.to_owned());
        write_instance_manifest(&deps.platform, &paths, &manifest)
    })
    .await?
    .map_err(|error| {
        SupervisorError::internal(format!(
            "the instance manifest could not record the initialization: {error}"
        ))
    })
}

#[cfg(test)]
mod tests {
    use stackly_platform::SecretValue;

    use super::*;

    const SECRET: &str = "correct-horse-battery-staple";

    fn redactor() -> Redactor {
        let redactor = Redactor::new();
        redactor.learn(&SecretValue::new(SECRET.to_owned()).unwrap());
        redactor
    }

    fn texts(lines: &[LogLine]) -> Vec<&str> {
        lines.iter().map(|line| line.text.as_str()).collect()
    }

    #[test]
    fn whole_lines_are_redacted_and_only_a_last_line_without_a_newline_is_partial() {
        let bytes = format!("one\r\nthe password {SECRET}\n\nlast").into_bytes();

        let lines = output_lines(&redactor(), &bytes, 1024, 64);

        assert_eq!(texts(&lines), ["one", "the password ****", "", "last"]);
        let partial: Vec<bool> = lines.iter().map(|line| line.partial).collect();
        assert_eq!(partial, [false, false, false, true]);
        assert!(lines.iter().all(|line| !line.continued));
    }

    #[test]
    fn a_line_longer_than_the_limit_is_cut_into_continued_pieces() {
        let lines = output_lines(&redactor(), "abcdéfg\n".as_bytes(), 1024, 5);

        assert_eq!(texts(&lines), ["abcd", "éfg"]);
        let continued: Vec<bool> = lines.iter().map(|line| line.continued).collect();
        assert_eq!(continued, [false, true]);
    }

    #[test]
    fn a_tail_as_long_as_the_capture_limit_drops_its_first_line() {
        // The tail starts inside the secret, so its end would escape the
        // redactor if the first line were kept.
        let bytes = format!("{}\nnext\n", &SECRET[10..]).into_bytes();

        let lines = output_lines(&redactor(), &bytes, bytes.len(), 64);

        assert_eq!(texts(&lines), ["next"]);
    }

    #[test]
    fn an_empty_stream_has_no_lines() {
        assert!(output_lines(&redactor(), b"", 1024, 64).is_empty());
    }
}
