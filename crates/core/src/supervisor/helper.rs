//! Running a helper to its end under a time limit (spec 0010, *Aturan
//! pembantu*): an `exec` probe now, init steps and stop and reload
//! commands later.
//!
//! A helper that runs past its limit gets `SIGTERM` to its process group,
//! then `SIGKILL` to the group after the grace period when it has not
//! exited. Only the platform's [`HelperHandle`] reaches
//! `terminate_helper_group`, so no signal can reach a main process. Every
//! platform call runs on the blocking pool.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use stackly_platform::{
    ChildRole, ExitListener, ExitNotice, HelperHandle, OutputTarget, Platform, ProcessError,
    SpawnRequest, StdinSource,
};
use tokio::sync::oneshot;

/// One helper to run: no lock, stdin from `/dev/null`, output captured.
#[derive(Debug, Clone)]
pub(crate) struct HelperCommand {
    /// The absolute executable from the plan.
    pub(crate) program: PathBuf,
    /// The arguments after the program.
    pub(crate) args: Vec<String>,
    /// The whole environment.
    pub(crate) env: Vec<(String, String)>,
    /// The working directory.
    pub(crate) cwd: PathBuf,
    /// Why it runs.
    pub(crate) role: ChildRole,
}

/// How a helper ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HelperEnd {
    /// It exited within its limit.
    Exited(ExitNotice),
    /// It ran past its limit and was terminated by the helper rule.
    TimedOut,
}

/// Spawns `command`, waits up to `limit` for it to exit, and applies the
/// helper rule when it does not: `SIGTERM` to its group, then `SIGKILL`
/// after `grace`, then a last wait of `grace` for the exit.
///
/// # Errors
///
/// Returns the [`ProcessError`] of the spawn, or of `watch_exit` (the
/// helper is then killed at once, since nothing would hear its exit).
pub(crate) async fn run_helper(
    platform: &Platform,
    command: HelperCommand,
    limit: Duration,
    grace: Duration,
) -> Result<HelperEnd, ProcessError> {
    let (sender, mut exit) = oneshot::channel();
    let listener = Arc::new(ExitSender(Mutex::new(Some(sender))));
    let spawner = platform.clone();
    let handle = blocking("spawn_helper", move || {
        let processes = spawner.processes();
        let handle = processes.spawn_helper(SpawnRequest {
            program: &command.program,
            args: &command.args,
            env: &command.env,
            cwd: &command.cwd,
            stdin: StdinSource::Null,
            output: OutputTarget::Capture,
            lock: None,
            role: command.role,
        })?;
        if let Err(error) = processes.watch_exit(handle.pid(), listener) {
            let _ = processes.terminate_helper_group(&handle, true);
            return Err(error);
        }
        Ok(handle)
    })
    .await?;

    if let Ok(notice) = tokio::time::timeout(limit, &mut exit).await {
        return notice.map(HelperEnd::Exited).map_err(|_| {
            internal(
                "watch_exit",
                format!("the watcher of helper {} dropped its exit", handle.pid()),
            )
        });
    }
    let handle = Arc::new(handle);
    terminate(platform, Arc::clone(&handle), false).await;
    if tokio::time::timeout(grace, &mut exit).await.is_err() {
        terminate(platform, Arc::clone(&handle), true).await;
        if tokio::time::timeout(grace, &mut exit).await.is_err() {
            tracing::warn!(
                pid = %handle.pid(),
                limit_ms = limit.as_millis(),
                "a helper killed after its time limit has not reported its exit"
            );
        }
    }
    Ok(HelperEnd::TimedOut)
}

/// Sends `SIGTERM`, or `SIGKILL` when `force`, to the helper's group. A
/// helper already reaped is fine: it is gone.
async fn terminate(platform: &Platform, handle: Arc<HelperHandle>, force: bool) {
    let platform = platform.clone();
    let pid = handle.pid();
    let result = blocking("terminate_helper_group", move || {
        platform.processes().terminate_helper_group(&handle, force)
    })
    .await;
    if let Err(error) = result
        && !matches!(error, ProcessError::NotFound { .. })
    {
        tracing::warn!(%pid, force, %error, "terminating a helper group failed");
    }
}

/// Runs the platform call `op` on the blocking pool.
async fn blocking<T: Send + 'static>(
    op: &'static str,
    call: impl FnOnce() -> Result<T, ProcessError> + Send + 'static,
) -> Result<T, ProcessError> {
    tokio::task::spawn_blocking(call)
        .await
        .unwrap_or_else(|error| Err(internal(op, format!("the blocking task failed: {error}"))))
}

/// A failure of the machinery around a platform call, not of the OS.
fn internal(op: &'static str, message: String) -> ProcessError {
    ProcessError::Io {
        op,
        path: None,
        kind: std::io::ErrorKind::Other,
        message,
    }
}

/// Hands one exit notice to the waiting task.
struct ExitSender(Mutex<Option<oneshot::Sender<ExitNotice>>>);

impl ExitListener for ExitSender {
    fn exited(&self, notice: ExitNotice) {
        let sender = self.0.lock().unwrap_or_else(PoisonError::into_inner).take();
        if let Some(sender) = sender {
            // The waiter may have given up; then nobody needs the notice.
            let _ = sender.send(notice);
        }
    }
}
