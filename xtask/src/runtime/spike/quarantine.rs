//! The quarantine probe (spec 0003, AC-9): an APFS clone of the artifact
//! whose every file carries `com.apple.quarantine`, run once per digest
//! from the same LaunchAgent. The original copy is never touched.

use std::path::Path;
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use uuid::Uuid;

use super::archive::walk;
use super::process::{self, CleanEnv};
use super::result::Quarantine;

const CP: &str = "/bin/cp";
const XATTR: &str = "/usr/bin/xattr";
/// How long the quarantined version command may run.
pub const QUARANTINE_TIMEOUT: Duration = Duration::from_secs(30);
/// Files per `xattr` call.
const XATTR_BATCH: usize = 200;

/// Clones `root` to `clone` with `cp -Rc`, marks every file of the clone
/// as quarantined, and runs the main executable with `args`.
pub fn probe(
    root: &Path,
    clone: &Path,
    main: &str,
    args: &[&str],
    env: &CleanEnv,
) -> Result<Quarantine> {
    let status = Command::new(CP)
        .arg("-Rc")
        .arg(root)
        .arg(clone)
        .status()
        .with_context(|| format!("cannot run {CP}"))?;
    if !status.success() {
        bail!("cp -Rc cannot clone the artifact");
    }
    let epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default();
    let value = format!(
        "0083;{epoch:x};StacklySpike;{}",
        Uuid::now_v7().hyphenated().to_string().to_uppercase()
    );
    let files: Vec<_> = walk(clone)?
        .into_iter()
        .filter(|entry| entry.meta.is_file())
        .map(|entry| entry.path)
        .collect();
    for batch in files.chunks(XATTR_BATCH) {
        let status = Command::new(XATTR)
            .args(["-w", "com.apple.quarantine", &value])
            .args(batch)
            .status()
            .with_context(|| format!("cannot run {XATTR}"))?;
        if !status.success() {
            bail!("xattr cannot mark the clone as quarantined");
        }
    }
    let mut command = env.command(&clone.join(main));
    command.args(args);
    Ok(match process::run(command, QUARANTINE_TIMEOUT) {
        Err(err) => Quarantine {
            outcome: "blocked".to_owned(),
            detail: format!("spawn refused: {err:#}"),
        },
        Ok(finished) => classify(&finished),
    })
}

fn classify(finished: &process::Finished) -> Quarantine {
    let (outcome, detail) = match finished.status {
        None => (
            "timeout",
            format!("still running after {} s", QUARANTINE_TIMEOUT.as_secs()),
        ),
        Some(status) if status.success() => ("ran", "exit 0".to_owned()),
        Some(status) => match process::code_and_signal(status) {
            (None, Some(signal)) => ("killed", format!("signal {signal}")),
            _ => (
                "blocked",
                format!("{}: {}", process::describe(status), finished.complaint()),
            ),
        },
    };
    Quarantine {
        outcome: outcome.to_owned(),
        detail,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::process::ExitStatusExt;
    use std::process::ExitStatus;

    fn finished(status: Option<ExitStatus>) -> process::Finished {
        process::Finished {
            status,
            stdout: String::new(),
            stderr: "denied".to_owned(),
        }
    }

    #[test]
    fn outcomes_follow_the_exit_status() {
        assert_eq!(
            classify(&finished(Some(ExitStatus::from_raw(0)))).outcome,
            "ran"
        );
        assert_eq!(
            classify(&finished(Some(ExitStatus::from_raw(1 << 8)))).outcome,
            "blocked"
        );
        let killed = classify(&finished(Some(ExitStatus::from_raw(9))));
        assert_eq!(
            (killed.outcome.as_str(), killed.detail.as_str()),
            ("killed", "signal 9")
        );
        assert_eq!(classify(&finished(None)).outcome, "timeout");
    }
}
