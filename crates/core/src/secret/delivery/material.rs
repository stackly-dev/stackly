//! Secret material on disk: the files of an instance's `secrets/` folder in
//! the runtime root, `<runtime>/i/<short-key>/secrets/` (mode 0700, every
//! file 0600). The runtime root holds only files that can be made again,
//! so no backup, Time Machine, or diagnostics bundle ever copies them.

use std::collections::BTreeSet;
use std::fmt;
use std::path::{Path, PathBuf};

use stackly_domain::{InstanceLifecycle, ShortKey};
use stackly_platform::{FileKind, Platform, PlatformError, RemoveReport, SecretValue};

use crate::error::CoreError;
use crate::layout::{PreparedLayout, ensure_instance_run_dir, ensure_private_dir};
use crate::ownership::remove_owned_tree;
use crate::provider::{COMPONENT_RULE, is_component};

/// Error returned by [`SecretFile::create`]. It never holds the value.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum DeliveryError {
    /// The file name is not one literal path component.
    #[error("invalid secret file name {name:?}: {COMPONENT_RULE}")]
    InvalidFileName {
        /// The name as given.
        name: String,
    },
    /// A folder or the file could not be made.
    #[error(transparent)]
    Core(#[from] CoreError),
}

/// A one time secret file (mechanism 3): `<runtime>/i/<short-key>/secrets/<name>`
/// with mode 0600, written atomically, in a folder with mode 0700. The
/// guard removes the file when it drops, so the file is gone once the step
/// that reads it ends, whether it succeeded, failed, or panicked (for
/// example `initdb --pwfile`).
pub struct SecretFile {
    platform: Platform,
    prepared: PreparedLayout,
    path: PathBuf,
    removed: bool,
}

impl SecretFile {
    /// Writes `value` to the instance's `secrets/` folder as `name`,
    /// creating the run folder and the `secrets/` folder with mode 0700
    /// when they are missing. An existing file of that name is replaced.
    ///
    /// `name` is one literal path component, `[A-Za-z0-9._-]{1,64}` and not
    /// `.` or `..`, the rule spec 0009 gives a `materializeSecret` name.
    ///
    /// # Errors
    ///
    /// Returns [`DeliveryError::InvalidFileName`] for a name that breaks
    /// the rule, and [`DeliveryError::Core`] when a folder on the way is not
    /// a private folder of the current user or a port fails.
    pub fn create(
        platform: &Platform,
        prepared: &PreparedLayout,
        key: ShortKey,
        name: &str,
        value: &SecretValue,
    ) -> Result<Self, DeliveryError> {
        if !is_component(name) {
            return Err(DeliveryError::InvalidFileName {
                name: name.to_owned(),
            });
        }
        let dir = ensure_secrets_dir(platform, prepared, key)?;
        let path = dir.join(name);
        platform
            .fs()
            .write_file_atomic(&path, value.expose().as_bytes())
            .map_err(CoreError::from)?;
        Ok(Self {
            platform: platform.clone(),
            prepared: prepared.clone(),
            path,
            removed: false,
        })
    }

    /// Returns the file's path, for the step that reads it.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Removes the file now and reports what happened, instead of leaving
    /// it to the drop, which can only log a failure.
    ///
    /// # Errors
    ///
    /// Returns the [`CoreError`] of [`remove_owned_tree`].
    pub fn remove(mut self) -> Result<RemoveReport, CoreError> {
        self.removed = true;
        remove_owned_tree(&self.platform, &self.prepared, &self.path)
    }
}

impl Drop for SecretFile {
    fn drop(&mut self) {
        if self.removed {
            return;
        }
        if let Err(err) = remove_owned_tree(&self.platform, &self.prepared, &self.path) {
            tracing::warn!(
                path = %self.path.display(),
                error = %err,
                "a secret file could not be removed; the next sweep removes it"
            );
        }
    }
}

impl fmt::Debug for SecretFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SecretFile")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

/// Makes sure `<runtime>/i/<key>/secrets/` exists as a private folder of
/// the current user and returns it.
fn ensure_secrets_dir(
    platform: &Platform,
    prepared: &PreparedLayout,
    key: ShortKey,
) -> Result<PathBuf, CoreError> {
    ensure_instance_run_dir(platform, prepared, key)?;
    let user = platform.identity().current_user()?;
    let dir = prepared.layout().instance_secrets_dir(key);
    ensure_private_dir(platform.fs(), &dir, &user)?;
    Ok(dir)
}

/// Removes an instance's whole `secrets/` folder: its plaintext configs
/// (mechanism 5) and any one time file a crash left. The daemon calls it
/// after a clean stop. A folder that does not exist yields a report with
/// `missing` set.
///
/// # Errors
///
/// Returns the [`CoreError`] of [`remove_owned_tree`].
pub fn remove_secret_material(
    platform: &Platform,
    prepared: &PreparedLayout,
    key: ShortKey,
) -> Result<RemoveReport, CoreError> {
    remove_owned_tree(
        platform,
        prepared,
        &prepared.layout().instance_secrets_dir(key),
    )
}

/// Whether an instance in `lifecycle` counts as running for its secret
/// material: `starting`, `running`, `recovering`, `stopping`, or `unknown`
/// (spec 0010). Its `secrets/` folder is then left alone.
#[must_use]
pub const fn counts_as_running(lifecycle: InstanceLifecycle) -> bool {
    matches!(
        lifecycle,
        InstanceLifecycle::Starting
            | InstanceLifecycle::Running
            | InstanceLifecycle::Recovering
            | InstanceLifecycle::Stopping
            | InstanceLifecycle::Unknown
    )
}

/// What one [`sweep_secret_material`] did, each list in short key order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct SweepReport {
    /// Run folders whose `secrets/` folder was removed.
    pub swept: Vec<ShortKey>,
    /// Run folders of instances that count as running, left alone.
    pub kept: Vec<ShortKey>,
    /// Run folders whose `secrets/` folder could not be removed. The sweep
    /// went on with the others.
    pub failed: Vec<(ShortKey, CoreError)>,
}

/// Removes the `secrets/` folder of every run folder in `<runtime>/i/`
/// whose instance does not count as running ([`counts_as_running`]). The
/// daemon calls it at start, during reconcile, with every instance it knows
/// and its lifecycle; a run folder of an instance it does not know is
/// swept too. Only `secrets/` folders are removed, never a run folder or
/// anything else in it, and an entry of `i/` that is not a directory named
/// by a short key is skipped. A missing `i/` folder sweeps nothing.
///
/// # Errors
///
/// Returns [`CoreError::Platform`] when `<runtime>/i/` cannot be read.
/// A run folder that fails is reported in [`SweepReport::failed`] instead.
pub fn sweep_secret_material<I>(
    platform: &Platform,
    prepared: &PreparedLayout,
    instances: I,
) -> Result<SweepReport, CoreError>
where
    I: IntoIterator<Item = (ShortKey, InstanceLifecycle)>,
{
    let running: BTreeSet<ShortKey> = instances
        .into_iter()
        .filter(|(_, lifecycle)| counts_as_running(*lifecycle))
        .map(|(key, _)| key)
        .collect();
    let entries = match platform
        .fs()
        .read_dir(&prepared.layout().instance_runs_dir())
    {
        Ok(entries) => entries,
        Err(PlatformError::NotFound { .. }) => return Ok(SweepReport::default()),
        Err(err) => return Err(err.into()),
    };
    let keys: BTreeSet<ShortKey> = entries
        .iter()
        .filter(|entry| entry.info.kind == FileKind::Dir)
        .filter_map(|entry| entry.name.to_str()?.parse().ok())
        .collect();
    let mut report = SweepReport::default();
    for key in keys {
        if running.contains(&key) {
            report.kept.push(key);
            continue;
        }
        match remove_secret_material(platform, prepared, key) {
            Ok(removed) if removed.missing => {}
            Ok(_) => report.swept.push(key),
            Err(err) => report.failed.push((key, err)),
        }
    }
    Ok(report)
}
