//! `instance.prepare_terminal` (spec 0070, AC-4): writes the Terminal
//! script of one `terminal` Open In action into the `temp` folder of the
//! instance and answers with its path. The GUI opens the script with
//! Terminal.app; nothing here spawns a process.
//!
//! The action is rendered again from the pinned package through the same
//! detail build as `instance.get_detail`, so a client names an action and
//! never sends a command. The script holds no secret: `openWith` templates
//! cannot name one (the loader refuses `secret.*`).

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use stackly_core::instance::open_with::{OpenActionState, OpenTarget};
use stackly_core::provider::OpenWithKind;
use stackly_domain::{InstanceLifecycle, ReadinessState};
use stackly_protocol::ErrorBody;
use stackly_protocol::codes::instance as codes;
use stackly_protocol::instance::open_with::terminal_script;
use stackly_protocol::instance::{PrepareTerminalParams, PrepareTerminalReply};

use super::detail::build_detail;
use super::unexpected;
use crate::dispatch::HandlerFuture;
use crate::map;
use crate::state::DaemonCore;

/// The mode of a script: the user may read, write, and run it, nobody else
/// may touch it.
const SCRIPT_MODE: u32 = 0o700;

/// `instance.prepare_terminal`.
pub fn prepare_terminal(
    core: &DaemonCore,
    params: PrepareTerminalParams,
) -> HandlerFuture<'_, PrepareTerminalReply> {
    Box::pin(async move {
        let supervisor = core.supervision().supervisor()?;
        let (detail, instance) = build_detail(core, params.instance_id).await?;
        let terminal = |kind: OpenWithKind| kind == OpenWithKind::Terminal;
        let Some(action) = detail
            .open_actions
            .iter()
            .find(|action| action.name == params.action && terminal(action.kind))
        else {
            let available: Vec<&str> = detail
                .open_actions
                .iter()
                .filter(|action| terminal(action.kind))
                .map(|action| action.name.as_str())
                .collect();
            return Err(Box::new(
                ErrorBody::new(
                    &codes::OPEN_ACTION_NOT_FOUND,
                    "the instance has no terminal action with that name",
                )
                .with_detail("action", params.action.clone())
                .with_detail("available", available.join(",")),
            ));
        };
        let command = match &action.state {
            OpenActionState::Available(OpenTarget::Terminal(command)) => command,
            OpenActionState::Unavailable(why) => {
                return Err(unavailable(&params.action, open_reason(*why)));
            }
            _ => return Err(unavailable(&params.action, "package_unavailable")),
        };
        if instance.lifecycle() != InstanceLifecycle::Running {
            return Err(unavailable(&params.action, "not_running"));
        }
        if supervisor.status(instance.id()).readiness.state() != ReadinessState::Ready {
            return Err(unavailable(&params.action, "not_ready"));
        }
        let script = terminal_script(
            instance.id().as_uuid(),
            &action.title,
            &map::detail::open_command(command),
        );
        let temp = core.prepared().instance_paths(instance.id()).temp;
        let path = temp.join(format!("open-{}.command", params.action));
        let target = path.clone();
        tokio::task::spawn_blocking(move || write_script(&target, script.as_bytes()))
            .await
            .map_err(|_| unexpected("the terminal script task failed"))?
            .map_err(|error| {
                tracing::warn!(%error, action = %params.action, "terminal script not written");
                Box::new(
                    ErrorBody::new(
                        &codes::OPEN_SCRIPT_UNWRITABLE,
                        "the terminal script could not be written",
                    )
                    .with_detail("reason", io_reason(&error)),
                )
            })?;
        Ok(PrepareTerminalReply {
            script_path: path.to_string_lossy().into_owned(),
        })
    })
}

/// `instance.open_action_unavailable` for `action` with `reason`.
fn unavailable(action: &str, reason: &str) -> Box<ErrorBody> {
    Box::new(
        ErrorBody::new(
            &codes::OPEN_ACTION_UNAVAILABLE,
            "the action can't be opened now",
        )
        .with_detail("action", action)
        .with_detail("reason", reason),
    )
}

/// The `reason` detail of an entry the daemon could not render.
fn open_reason(why: stackly_core::instance::connection::ConnectionUnavailable) -> &'static str {
    use stackly_core::instance::connection::ConnectionUnavailable;
    match why {
        ConnectionUnavailable::Retained => "retained",
        ConnectionUnavailable::PortNotAssigned => "port_not_assigned",
        _ => "package_unavailable",
    }
}

/// A short, value free reason for a failed write.
fn io_reason(error: &std::io::Error) -> &'static str {
    match error.kind() {
        std::io::ErrorKind::NotFound => "temp_folder_missing",
        std::io::ErrorKind::PermissionDenied => "permission_denied",
        std::io::ErrorKind::StorageFull => "no_space",
        _ => "io_error",
    }
}

/// Writes `bytes` to `path` atomically with mode 0700: a new temporary
/// file `<name>.<uuid v7>.tmp` in the same folder, synced and renamed over
/// `path`. On failure the temporary file is removed, so no half written
/// script is ever left.
fn write_script(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let temp = temp_name(path);
    let written = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(SCRIPT_MODE)
            .open(&temp)?;
        file.write_all(bytes)?;
        // The umask may have narrowed the mode at create time.
        file.set_permissions(std::os::unix::fs::PermissionsExt::from_mode(SCRIPT_MODE))?;
        file.sync_all()?;
        fs::rename(&temp, path)
    })();
    if written.is_err() {
        let _ = fs::remove_file(&temp);
    }
    written
}

/// The temporary file next to `path`.
fn temp_name(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".{}.tmp", uuid::Uuid::now_v7()));
    path.with_file_name(name)
}
