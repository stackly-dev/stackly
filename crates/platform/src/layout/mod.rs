//! The per user directory layout: four owned roots and every path derived
//! from them.

mod root_override;
mod roots;

use std::path::{Path, PathBuf};

use stackly_domain::{
    EnvironmentId, ExactVersion, InstanceId, InstancePathEntry, InstancePathKind, LogicalRoot,
    ProviderRevisionKey, RelativePath, Sha256Digest, ShortKey, SoftwareName, Target,
};

pub use root_override::{ROOT_OVERRIDE_ENV, validate_root_override};
pub use roots::{
    DirectoryResolver, FixedRoots, RUNTIME_ROOT_MAX, RootKind, RootSet, SOCKET_NAME_MAX,
    SOCKET_PATH_MAX, default_runtime_root,
};

/// File name of the metadata file at the root of every instance folder.
pub const INSTANCE_MANIFEST_FILE: &str = "instance-manifest.json";
/// Folder under the runtime root that holds one run folder per instance.
/// One letter, because it counts against the socket path budget.
const INSTANCE_RUNS_DIR: &str = "i";
/// Folder name used under both `data` and `logs` for per instance folders.
const INSTANCES_DIR: &str = "instances";
/// The software's data directory inside an instance folder.
const INSTANCE_DATA_DIR: &str = "data";
/// The rendered configuration inside an instance folder.
const INSTANCE_CONFIG_DIR: &str = "config";
/// Earlier configurations inside an instance folder.
const INSTANCE_CONFIG_HISTORY_DIR: &str = "config-history";
/// Scratch space inside an instance folder.
const INSTANCE_TEMP_DIR: &str = "temp";
/// Folder under the data root that holds the registry.
const REGISTRY_DIR: &str = "registry";
/// Folder under the data root that holds the file keychain of a root
/// override (spec 0011).
const KEYCHAIN_DIR: &str = "keychain";
/// Folder inside an instance's run folder that holds its secret files
/// (spec 0011).
const INSTANCE_SECRETS_DIR: &str = "secrets";

/// Derives every Stackly path from a [`RootSet`], without any I/O.
///
/// Path components come from validated domain values (IDs, short keys,
/// names, versions, and digests), so no function here can fail, and no other
/// code joins Stackly paths by hand.
///
/// ```text
/// <data>/stacklyd.lock, registry/{pre-migration,recovery}/,
///        providers/<namespace>/<name>/<revision>/<manifest sha256>/,
///        runtimes/<software>/<version>/<os>-<arch>/<artifact sha256>/,
///        instances/<id>/, environments/<id>/, operations/, backups/,
///        retained/<id>/, keychain/
/// <logs>/daemon/, instances/<id>/
/// <cache>/downloads/
/// <runtime>/stacklyd.sock, i/<short key>/, i/<short key>/secrets/
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
        self.roots.data().join(REGISTRY_DIR)
    }

    /// Returns the registry database file.
    #[must_use]
    pub fn registry_db(&self) -> PathBuf {
        self.registry_dir().join("stackly.sqlite")
    }

    /// Returns the folder that holds the verified registry snapshots taken
    /// before a migration, `registry/pre-migration/`.
    #[must_use]
    pub fn registry_snapshots_dir(&self) -> PathBuf {
        self.registry_dir().join("pre-migration")
    }

    /// Returns the folder that holds registry recovery reports,
    /// `registry/recovery/`.
    #[must_use]
    pub fn registry_recovery_dir(&self) -> PathBuf {
        self.registry_dir().join("recovery")
    }

    /// Returns the folder that holds the file keychain of a root override,
    /// `keychain/` (spec 0011). The Keychain adapter creates it only when
    /// `STACKLY_ROOT` is set; it is not a standard folder.
    #[must_use]
    pub fn keychain_dir(&self) -> PathBuf {
        self.roots.data().join(KEYCHAIN_DIR)
    }

    /// Returns the folder that holds provider revisions.
    #[must_use]
    pub fn providers_dir(&self) -> PathBuf {
        self.roots.data().join("providers")
    }

    /// Returns one provider revision's folder,
    /// `providers/<namespace>/<name>/<revision>/<manifest sha256>/`.
    #[must_use]
    pub fn provider_revision_dir(&self, key: &ProviderRevisionKey) -> PathBuf {
        self.providers_dir()
            .join(key.provider().namespace())
            .join(key.provider().name())
            .join(key.revision().as_str())
            .join(key.manifest_sha256().to_string())
    }

    /// Returns the folder that holds installed runtimes.
    #[must_use]
    pub fn runtimes_dir(&self) -> PathBuf {
        self.roots.data().join("runtimes")
    }

    /// Returns one managed runtime's folder,
    /// `runtimes/<software>/<version>/<os>-<arch>/<artifact sha256>/`.
    #[must_use]
    pub fn runtime_dir(
        &self,
        software: &SoftwareName,
        version: &ExactVersion,
        target: Target,
        artifact_sha256: &Sha256Digest,
    ) -> PathBuf {
        self.runtimes_dir()
            .join(software.as_str())
            .join(version.as_str())
            .join(target.to_string())
            .join(artifact_sha256.to_string())
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

    /// Returns the folder that holds one instance's secret files,
    /// `<runtime>/i/<short key>/secrets/` (spec 0011). It lives under the
    /// runtime root, so no backup ever copies it.
    #[must_use]
    pub fn instance_secrets_dir(&self, key: ShortKey) -> PathBuf {
        self.instance_run_dir(key).join(INSTANCE_SECRETS_DIR)
    }

    fn paths_under(&self, parent: &Path, id: InstanceId) -> InstancePaths {
        let root = parent.join(id.to_string());
        InstancePaths {
            manifest: root.join(INSTANCE_MANIFEST_FILE),
            data: root.join(INSTANCE_DATA_DIR),
            config: root.join(INSTANCE_CONFIG_DIR),
            config_history: root.join(INSTANCE_CONFIG_HISTORY_DIR),
            temp: root.join(INSTANCE_TEMP_DIR),
            logs: self.instance_log_dir(id),
            run: self.instance_run_dir(id.short_key()),
            root,
        }
    }
}

/// Returns the seven paths of an active instance as the registry stores
/// them, relative to a logical root, in [`InstancePathKind::ALL`] order:
///
/// | Kind | Root | Path |
/// | --- | --- | --- |
/// | `root` | data | `instances/<id>` |
/// | `data` | data | `instances/<id>/data` |
/// | `config` | data | `instances/<id>/config` |
/// | `config_history` | data | `instances/<id>/config-history` |
/// | `temp` | data | `instances/<id>/temp` |
/// | `logs` | logs | `instances/<id>` |
/// | `run` | runtime | `i/<short key>` |
///
/// It needs no roots: [`RootSet::resolve`] of each entry equals the
/// matching field of [`DirectoryLayout::instance_paths`].
#[must_use]
pub fn instance_path_entries(id: InstanceId) -> [InstancePathEntry; 7] {
    let instance = format!("{INSTANCES_DIR}/{id}");
    let inside = |dir: &str| format!("{instance}/{dir}");
    InstancePathKind::ALL.map(|kind| {
        let (root, path) = match kind {
            InstancePathKind::Root => (LogicalRoot::Data, instance.clone()),
            InstancePathKind::Data => (LogicalRoot::Data, inside(INSTANCE_DATA_DIR)),
            InstancePathKind::Config => (LogicalRoot::Data, inside(INSTANCE_CONFIG_DIR)),
            InstancePathKind::ConfigHistory => {
                (LogicalRoot::Data, inside(INSTANCE_CONFIG_HISTORY_DIR))
            }
            InstancePathKind::Temp => (LogicalRoot::Data, inside(INSTANCE_TEMP_DIR)),
            InstancePathKind::Logs => (LogicalRoot::Logs, instance.clone()),
            InstancePathKind::Run => (
                LogicalRoot::Runtime,
                format!("{INSTANCE_RUNS_DIR}/{}", id.short_key()),
            ),
        };
        let path = RelativePath::new(path)
            .expect("an instance path joins fixed folder names, an ID, and a short key");
        InstancePathEntry::new(kind, root, path)
    })
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
    const DIGEST: &str = "cb95f16d56a9ab7b938a03732ddf4ab691346124747fbb811ac6c09928f1fa6b";

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
    fn instance_path_entries_resolve_to_the_layout_paths() {
        let layout = layout();
        for _ in 0..1000 {
            let id = InstanceId::generate();
            let paths = layout.instance_paths(id);
            let entries = instance_path_entries(id);
            let resolved = |kind: InstancePathKind| {
                let entry = entries.iter().find(|entry| entry.kind() == kind).unwrap();
                layout.roots().resolve(entry.root(), entry.path())
            };

            let kinds: Vec<_> = entries.iter().map(InstancePathEntry::kind).collect();
            assert_eq!(kinds, InstancePathKind::ALL);
            assert_eq!(resolved(InstancePathKind::Root), paths.root);
            assert_eq!(resolved(InstancePathKind::Data), paths.data);
            assert_eq!(resolved(InstancePathKind::Config), paths.config);
            assert_eq!(
                resolved(InstancePathKind::ConfigHistory),
                paths.config_history
            );
            assert_eq!(resolved(InstancePathKind::Temp), paths.temp);
            assert_eq!(resolved(InstancePathKind::Logs), paths.logs);
            assert_eq!(resolved(InstancePathKind::Run), paths.run);
        }
    }

    #[test]
    fn instance_path_entries_of_one_id_are_the_documented_table() {
        let id: InstanceId = ID.parse().unwrap();
        let entries: Vec<_> = instance_path_entries(id)
            .iter()
            .map(|entry| {
                format!(
                    "{} {}:{}",
                    entry.kind(),
                    entry.root(),
                    entry.path().as_str()
                )
            })
            .collect();

        assert_eq!(
            entries,
            [
                format!("root data:instances/{ID}"),
                format!("data data:instances/{ID}/data"),
                format!("config data:instances/{ID}/config"),
                format!("config_history data:instances/{ID}/config-history"),
                format!("temp data:instances/{ID}/temp"),
                format!("logs logs:instances/{ID}"),
                "run runtime:i/718293a4".to_owned(),
            ]
        );
    }

    #[test]
    fn snapshot_and_recovery_folders_sit_in_the_registry_folder() {
        let layout = layout();

        assert_eq!(
            layout.registry_snapshots_dir(),
            layout.registry_dir().join("pre-migration")
        );
        assert_eq!(
            layout.registry_recovery_dir(),
            layout.registry_dir().join("recovery")
        );
    }

    #[test]
    fn the_keychain_folder_is_under_data_and_secret_files_under_the_run_folder() {
        let layout = layout();
        let id: InstanceId = ID.parse().unwrap();
        let roots = layout.roots();

        assert_eq!(layout.keychain_dir(), roots.data().join("keychain"));
        assert_eq!(
            layout.instance_secrets_dir(id.short_key()),
            roots.runtime().join("i").join("718293a4").join("secrets")
        );
        assert!(!layout.standard_dirs().contains(&layout.keychain_dir()));
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
    fn provider_revision_dir_nests_namespace_name_revision_and_digest() {
        let layout = layout();
        let key = ProviderRevisionKey::new(
            "official/postgresql".parse().unwrap(),
            "2026.10.1".parse().unwrap(),
            DIGEST.parse().unwrap(),
        );

        assert_eq!(
            layout.provider_revision_dir(&key),
            layout
                .roots()
                .data()
                .join("providers")
                .join("official")
                .join("postgresql")
                .join("2026.10.1")
                .join(DIGEST)
        );
    }

    #[test]
    fn runtime_dir_nests_software_version_target_and_digest() {
        let layout = layout();
        let dir = layout.runtime_dir(
            &"postgresql".parse().unwrap(),
            &"17.6".parse().unwrap(),
            "macos-aarch64".parse().unwrap(),
            &DIGEST.to_uppercase().parse().unwrap(),
        );

        assert_eq!(
            dir,
            layout
                .roots()
                .data()
                .join("runtimes")
                .join("postgresql")
                .join("17.6")
                .join("macos-aarch64")
                .join(DIGEST)
        );
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
