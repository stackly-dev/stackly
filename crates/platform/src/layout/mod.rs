//! The per user directory layout: four owned roots and every path derived
//! from them.

mod root_override;
mod roots;

use std::path::{Path, PathBuf};

use stackly_domain::{EnvironmentId, InstanceId, ShortKey};

pub use root_override::{ROOT_OVERRIDE_ENV, validate_root_override};
pub use roots::{
    DirectoryResolver, FixedRoots, RUNTIME_ROOT_MAX, RootKind, RootSet, SOCKET_NAME_MAX,
    SOCKET_PATH_MAX,
};

/// File name of the metadata file at the root of every instance folder.
pub const INSTANCE_MANIFEST_FILE: &str = "instance-manifest.json";
/// Folder under the runtime root that holds one run folder per instance.
/// One letter, because it counts against the socket path budget.
const INSTANCE_RUNS_DIR: &str = "i";
/// Folder name used under both `data` and `logs` for per instance folders.
const INSTANCES_DIR: &str = "instances";

/// Derives every Stackly path from a [`RootSet`], without any I/O.
///
/// Path components come from validated domain values (IDs, short keys), so
/// no function here can fail, and no other code joins Stackly paths by hand.
///
/// ```text
/// <data>/stacklyd.lock, registry/, providers/, runtimes/, instances/<id>/,
///        environments/<id>/, operations/, backups/, retained/<id>/
/// <logs>/daemon/, instances/<id>/
/// <cache>/downloads/
/// <runtime>/stacklyd.sock, i/<short key>/
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryLayout {
    roots: RootSet,
}

impl DirectoryLayout {
    /// Wraps a root set.
    #[must_use]
    pub const fn new(roots: RootSet) -> Self {
        Self { roots }
    }

    /// Returns the root set.
    #[must_use]
    pub const fn roots(&self) -> &RootSet {
        &self.roots
    }

    /// Returns the four owned roots in [`RootKind::ALL`] order.
    #[must_use]
    pub fn owned_roots(&self) -> [&Path; 4] {
        RootKind::ALL.map(|kind| self.roots.get(kind))
    }

    /// Returns every standard subfolder of the owned roots, the ones
    /// preparing the layout creates: `registry`, `providers`, `runtimes`,
    /// `instances`, `environments`, `operations`, `backups`, and `retained`
    /// under data; `daemon` and `instances` under logs; `downloads` under
    /// cache; and `i` under runtime.
    #[must_use]
    pub fn standard_dirs(&self) -> Vec<PathBuf> {
        vec![
            self.registry_dir(),
            self.providers_dir(),
            self.runtimes_dir(),
            self.instances_dir(),
            self.environments_dir(),
            self.operations_dir(),
            self.backups_dir(),
            self.retained_dir(),
            self.daemon_log_dir(),
            self.instance_logs_dir(),
            self.downloads_dir(),
            self.instance_runs_dir(),
        ]
    }

    /// Returns the lock file that keeps one daemon per user. It lives in the
    /// data root, because the OS may clean the runtime root while the daemon
    /// runs.
    #[must_use]
    pub fn daemon_lock(&self) -> PathBuf {
        self.roots.data().join("stacklyd.lock")
    }

    /// Returns the folder that holds the registry database.
    #[must_use]
    pub fn registry_dir(&self) -> PathBuf {
        self.roots.data().join("registry")
    }

    /// Returns the registry database file.
    #[must_use]
    pub fn registry_db(&self) -> PathBuf {
        self.registry_dir().join("stackly.sqlite")
    }

    /// Returns the folder that holds provider revisions.
    #[must_use]
    pub fn providers_dir(&self) -> PathBuf {
        self.roots.data().join("providers")
    }

    /// Returns the folder that holds installed runtimes.
    #[must_use]
    pub fn runtimes_dir(&self) -> PathBuf {
        self.roots.data().join("runtimes")
    }

    /// Returns the folder that holds active instance folders.
    #[must_use]
    pub fn instances_dir(&self) -> PathBuf {
        self.roots.data().join(INSTANCES_DIR)
    }

    /// Returns the paths of an active instance, under `instances/<id>/`.
    #[must_use]
    pub fn instance_paths(&self, id: InstanceId) -> InstancePaths {
        self.paths_under(&self.instances_dir(), id)
    }

    /// Returns the folder that holds retained instance folders.
    #[must_use]
    pub fn retained_dir(&self) -> PathBuf {
        self.roots.data().join("retained")
    }

    /// Returns the paths of a retained instance, under `retained/<id>/`.
    /// The tree is the same as for an active instance.
    #[must_use]
    pub fn retained_instance_paths(&self, id: InstanceId) -> InstancePaths {
        self.paths_under(&self.retained_dir(), id)
    }

    /// Returns the folder that holds environment folders.
    #[must_use]
    pub fn environments_dir(&self) -> PathBuf {
        self.roots.data().join("environments")
    }

    /// Returns one environment's folder.
    #[must_use]
    pub fn environment_dir(&self, id: EnvironmentId) -> PathBuf {
        self.environments_dir().join(id.to_string())
    }

    /// Returns the folder that holds operation journals.
    #[must_use]
    pub fn operations_dir(&self) -> PathBuf {
        self.roots.data().join("operations")
    }

    /// Returns the folder that holds backups.
    #[must_use]
    pub fn backups_dir(&self) -> PathBuf {
        self.roots.data().join("backups")
    }

    /// Returns the daemon's log folder.
    #[must_use]
    pub fn daemon_log_dir(&self) -> PathBuf {
        self.roots.logs().join("daemon")
    }

    /// Returns the folder that holds one log folder per instance.
    #[must_use]
    pub fn instance_logs_dir(&self) -> PathBuf {
        self.roots.logs().join(INSTANCES_DIR)
    }

    /// Returns one instance's log folder, `logs/instances/<id>/`.
    #[must_use]
    pub fn instance_log_dir(&self, id: InstanceId) -> PathBuf {
        self.instance_logs_dir().join(id.to_string())
    }

    /// Returns the folder for downloads.
    #[must_use]
    pub fn downloads_dir(&self) -> PathBuf {
        self.roots.cache().join("downloads")
    }

    /// Returns the daemon's Unix socket.
    #[must_use]
    pub fn daemon_socket(&self) -> PathBuf {
        self.roots.runtime().join("stacklyd.sock")
    }

    /// Returns the folder that holds one run folder per instance.
    #[must_use]
    pub fn instance_runs_dir(&self) -> PathBuf {
        self.roots.runtime().join(INSTANCE_RUNS_DIR)
    }

    /// Returns one instance's run folder, `<runtime>/i/<short key>/`, for
    /// its sockets and PID file. With a socket name of at most
    /// [`SOCKET_NAME_MAX`] bytes the socket path fits [`SOCKET_PATH_MAX`].
    #[must_use]
    pub fn instance_run_dir(&self, key: ShortKey) -> PathBuf {
        self.instance_runs_dir().join(key.to_string())
    }

    fn paths_under(&self, parent: &Path, id: InstanceId) -> InstancePaths {
        let root = parent.join(id.to_string());
        InstancePaths {
            manifest: root.join(INSTANCE_MANIFEST_FILE),
            data: root.join("data"),
            config: root.join("config"),
            config_history: root.join("config-history"),
            temp: root.join("temp"),
            logs: self.instance_log_dir(id),
            run: self.instance_run_dir(id.short_key()),
            root,
        }
    }
}

/// Every path of one instance.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct InstancePaths {
    /// The instance folder, `instances/<id>/` or `retained/<id>/`.
    pub root: PathBuf,
    /// The `instance-manifest.json` file.
    pub manifest: PathBuf,
    /// The software's data directory.
    pub data: PathBuf,
    /// The rendered configuration.
    pub config: PathBuf,
    /// Earlier configurations.
    pub config_history: PathBuf,
    /// Scratch space for the instance.
    pub temp: PathBuf,
    /// The log folder, `logs/instances/<id>/`.
    pub logs: PathBuf,
    /// The run folder under the runtime root, `i/<short key>/`.
    pub run: PathBuf,
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "01928c3e-6f1a-7b2c-8d4e-5f60718293a4";

    fn layout() -> DirectoryLayout {
        let base = if cfg!(windows) { r"C:\" } else { "/" };
        let base = Path::new(base);
        DirectoryLayout::new(
            RootSet::new(
                base.join("d"),
                base.join("l"),
                base.join("c"),
                base.join("r"),
            )
            .unwrap(),
        )
    }

    #[test]
    fn instance_paths_derive_from_the_id_and_its_short_key() {
        let layout = layout();
        let id: InstanceId = ID.parse().unwrap();
        let roots = layout.roots();
        let paths = layout.instance_paths(id);

        assert_eq!(paths.root, roots.data().join("instances").join(ID));
        assert_eq!(paths.manifest, paths.root.join("instance-manifest.json"));
        assert_eq!(paths.data, paths.root.join("data"));
        assert_eq!(paths.config, paths.root.join("config"));
        assert_eq!(paths.config_history, paths.root.join("config-history"));
        assert_eq!(paths.temp, paths.root.join("temp"));
        assert_eq!(paths.logs, roots.logs().join("instances").join(ID));
        assert_eq!(paths.run, roots.runtime().join("i").join("718293a4"));
    }

    #[test]
    fn retained_paths_keep_the_same_tree_logs_and_run_folder() {
        let layout = layout();
        let id: InstanceId = ID.parse().unwrap();
        let active = layout.instance_paths(id);
        let retained = layout.retained_instance_paths(id);

        assert_eq!(
            retained.root,
            layout.roots().data().join("retained").join(ID)
        );
        assert_eq!(
            retained.manifest,
            retained.root.join("instance-manifest.json")
        );
        assert_eq!(retained.logs, active.logs);
        assert_eq!(retained.run, active.run);
    }

    #[test]
    fn daemon_lock_is_in_the_data_root_and_socket_in_the_runtime_root() {
        let layout = layout();

        assert_eq!(
            layout.daemon_lock(),
            layout.roots().data().join("stacklyd.lock")
        );
        assert_eq!(
            layout.daemon_socket(),
            layout.roots().runtime().join("stacklyd.sock")
        );
        assert_eq!(
            layout.registry_db(),
            layout
                .roots()
                .data()
                .join("registry")
                .join("stackly.sqlite")
        );
    }

    #[test]
    fn standard_dirs_are_direct_children_of_an_owned_root() {
        let layout = layout();
        let dirs = layout.standard_dirs();

        assert_eq!(dirs.len(), 12);
        for dir in &dirs {
            let parent = dir.parent().unwrap();
            assert!(layout.owned_roots().contains(&parent), "{}", dir.display());
        }
        assert!(dirs.contains(&layout.roots().runtime().join("i")));
        assert!(dirs.contains(&layout.roots().cache().join("downloads")));
    }

    #[test]
    fn environment_dir_is_named_by_its_id() {
        let layout = layout();
        let id = EnvironmentId::generate();

        assert_eq!(
            layout.environment_dir(id),
            layout
                .roots()
                .data()
                .join("environments")
                .join(id.to_string())
        );
    }
}
