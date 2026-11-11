//! The per user directory layout: four owned roots and every path derived
//! from them.

mod locations;
mod root_override;
mod roots;

use std::path::{Path, PathBuf};

use stackly_domain::{
    EnvironmentId, ExactVersion, InstanceId, InstancePathEntry, InstancePathKind, LogicalRoot,
    ProviderRevisionKey, RelativePath, Sha256Digest, ShortKey, SoftwareName, StorageLocationId,
    Target,
};

use crate::process::OutputStream;

pub use locations::{LocationSet, UnknownLocation};
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
/// The folder of the durable log store inside an instance's log folder
/// (spec 0027). Provider log files may not use it as their first component
/// (spec 0009).
pub const CAPTURED_LOGS_DIR: &str = "captured";

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
/// The launch record in the data root (spec 0015).
const AGENT_LAUNCH_RECORD_FILE: &str = "background-agent.json";
/// File name of the record of the last daemon start that failed, in the
/// data root (spec 0034, *Catatan gagal start daemon*). Public, because
/// `stacklyd` names it under a data root it could not resolve fully.
pub const DAEMON_START_FAILURE_RECORD_FILE: &str = "daemon-start-failure.json";
/// Folder under the cache root that holds the staged bundle previews
/// (spec 0034).
const DIAGNOSTICS_CACHE_DIR: &str = "diagnostics";
/// Folder under the data root that holds the file keychain of a root
/// override (spec 0011).
const KEYCHAIN_DIR: &str = "keychain";
/// Folder inside an instance's run folder that holds its secret files
/// (spec 0011).
const INSTANCE_SECRETS_DIR: &str = "secrets";
/// The lock file at the root of every instance folder, which the main
/// process inherits at fd 3 (spec 0010).
const INSTANCE_LOCK_FILE: &str = "instance.lock";

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
/// <runtime>/stacklyd.sock, i/<short key>/,
///           i/<short key>/{stdout,stderr}.spool, i/<short key>/secrets/
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

    /// Returns the launch record `<data>/background-agent.json`, which only
    /// the daemon writes, and only when launchd started the production agent
    /// for the default root (spec 0015).
    #[must_use]
    pub fn agent_launch_record(&self) -> PathBuf {
        self.roots.data().join(AGENT_LAUNCH_RECORD_FILE)
    }

    /// Returns `<cache>/diagnostics`, the staging folder of diagnostic
    /// bundle previews, which the daemon empties at every start (spec 0034,
    /// *Staging preview*).
    #[must_use]
    pub fn diagnostics_cache_dir(&self) -> PathBuf {
        self.roots.cache().join(DIAGNOSTICS_CACHE_DIR)
    }

    /// Returns `<data>/daemon-start-failure.json`, which `stacklyd` writes
    /// when it exits 1 after it knew its data root and removes once a later
    /// start bound its socket (spec 0034, AC-11).
    #[must_use]
    pub fn daemon_start_failure_record(&self) -> PathBuf {
        self.roots.data().join(DAEMON_START_FAILURE_RECORD_FILE)
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

    /// Returns the store of imported local provider packages,
    /// `provider-imports/` under data (spec 0037, *Store salinan*). It is
    /// not a standard folder: the first import creates it.
    #[must_use]
    pub fn provider_imports_dir(&self) -> PathBuf {
        self.roots.data().join("provider-imports")
    }

    /// Returns the folder of one imported package copy, named by its
    /// package digest: `provider-imports/<package sha256>/`.
    #[must_use]
    pub fn provider_import_dir(&self, package_sha256: &Sha256Digest) -> PathBuf {
        self.provider_imports_dir().join(package_sha256.to_string())
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

    /// Returns the paths of an active instance, under `instances/<id>/`
    /// of the built in data location and `instances/<id>/` of the built in
    /// log location.
    #[must_use]
    pub fn instance_paths(&self, id: InstanceId) -> InstancePaths {
        self.instance_paths_in(id, self.roots.data(), self.roots.logs())
    }

    /// Returns the paths of an active instance whose data lives in the
    /// location folder `data` and whose logs live in the location folder
    /// `logs`: the folders [`LocationSet::base`] gives. The relative paths
    /// are the same in every location (spec 0032).
    #[must_use]
    pub fn instance_paths_in(&self, id: InstanceId, data: &Path, logs: &Path) -> InstancePaths {
        self.paths_under(&data.join(INSTANCES_DIR), &logs.join(INSTANCES_DIR), id)
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
        self.retained_instance_paths_in(id, self.roots.data(), self.roots.logs())
    }

    /// Returns the paths of a retained instance in the location folders
    /// `data` and `logs`: `retained/<id>/` under `data`.
    #[must_use]
    pub fn retained_instance_paths_in(
        &self,
        id: InstanceId,
        data: &Path,
        logs: &Path,
    ) -> InstancePaths {
        self.paths_under(&data.join("retained"), &logs.join(INSTANCES_DIR), id)
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

    /// Returns the durable log store of one instance,
    /// `logs/instances/<id>/captured/` (spec 0027). The log writer creates
    /// it with `create_dir_private`; `prepare_layout` does not.
    #[must_use]
    pub fn instance_captured_logs_dir(&self, id: InstanceId) -> PathBuf {
        self.instance_log_dir(id).join(CAPTURED_LOGS_DIR)
    }

    /// Returns the folder for downloads in the built in cache location.
    #[must_use]
    pub fn downloads_dir(&self) -> PathBuf {
        Self::downloads_dir_in(self.roots.cache())
    }

    /// Returns the folder for downloads in the cache location folder
    /// `location`: `downloads/` under it (spec 0032).
    #[must_use]
    pub fn downloads_dir_in(location: &Path) -> PathBuf {
        location.join("downloads")
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
    /// its sockets, output spools, and secret files. With a socket name of
    /// at most
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

    /// Returns the spool of one output stream of an instance's main
    /// process, `<runtime>/i/<short key>/stdout.spool` or `stderr.spool`
    /// (spec 0010). Raw output stays under the runtime root, never in the
    /// data root or the durable logs.
    #[must_use]
    pub fn instance_spool(&self, key: ShortKey, stream: OutputStream) -> PathBuf {
        self.instance_run_dir(key).join(stream.spool_file_name())
    }

    fn paths_under(&self, parent: &Path, logs_parent: &Path, id: InstanceId) -> InstancePaths {
        let root = parent.join(id.to_string());
        InstancePaths {
            manifest: root.join(INSTANCE_MANIFEST_FILE),
            lock: root.join(INSTANCE_LOCK_FILE),
            data: root.join(INSTANCE_DATA_DIR),
            config: root.join(INSTANCE_CONFIG_DIR),
            config_history: root.join(INSTANCE_CONFIG_HISTORY_DIR),
            temp: root.join(INSTANCE_TEMP_DIR),
            logs: logs_parent.join(id.to_string()),
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
/// Every entry but `run` names the storage location that holds it: `data`
/// for the five data kinds and `logs` for the log folder (spec 0032). The
/// relative paths are the same in every location. For the built in
/// locations, [`RootSet::resolve`] of each entry equals the matching field
/// of [`DirectoryLayout::instance_paths`]; a custom location resolves
/// through the location set (step 3 of spec 0032).
#[must_use]
pub fn instance_path_entries(
    id: InstanceId,
    data: StorageLocationId,
    logs: StorageLocationId,
) -> [InstancePathEntry; 7] {
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
        let entry = InstancePathEntry::new(kind, root, path);
        match root {
            LogicalRoot::Data => entry.in_location(data),
            LogicalRoot::Logs => entry.in_location(logs),
            LogicalRoot::Runtime => entry,
        }
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
    /// The `instance.lock` file: the lock on the instance's data, which the
    /// main process inherits and which moves with the folder when it is
    /// retained (spec 0010).
    pub lock: PathBuf,
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
    use stackly_domain::LocationKind;

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

    fn built_in_entries(id: InstanceId) -> [InstancePathEntry; 7] {
        instance_path_entries(
            id,
            LocationKind::Data.built_in_id(),
            LocationKind::Logs.built_in_id(),
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
        assert_eq!(paths.lock, paths.root.join("instance.lock"));
        assert_eq!(paths.data, paths.root.join("data"));
        assert_eq!(paths.config, paths.root.join("config"));
        assert_eq!(paths.config_history, paths.root.join("config-history"));
        assert_eq!(paths.temp, paths.root.join("temp"));
        assert_eq!(paths.logs, roots.logs().join("instances").join(ID));
        assert_eq!(paths.run, roots.runtime().join("i").join("718293a4"));
    }

    #[test]
    fn spools_live_in_the_run_folder_and_the_lock_moves_with_a_retained_folder() {
        let layout = layout();
        let id: InstanceId = ID.parse().unwrap();
        let run = layout.instance_paths(id).run;

        assert_eq!(
            layout.instance_spool(id.short_key(), OutputStream::Stdout),
            run.join("stdout.spool")
        );
        assert_eq!(
            layout.instance_spool(id.short_key(), OutputStream::Stderr),
            run.join("stderr.spool")
        );
        let retained = layout.retained_instance_paths(id);
        assert_eq!(retained.lock, retained.root.join("instance.lock"));
        assert!(retained.lock.starts_with(layout.retained_dir()));
    }

    #[test]
    fn instance_path_entries_resolve_to_the_layout_paths() {
        let layout = layout();
        for _ in 0..1000 {
            let id = InstanceId::generate();
            let paths = layout.instance_paths(id);
            let entries = built_in_entries(id);
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
    fn data_kinds_name_the_data_location_the_logs_kind_the_log_location_and_run_none() {
        let id: InstanceId = ID.parse().unwrap();
        let data = StorageLocationId::generate();
        let logs = StorageLocationId::generate();
        let locations: Vec<_> = instance_path_entries(id, data, logs)
            .iter()
            .map(|entry| (entry.kind(), entry.location()))
            .collect();

        assert_eq!(
            locations,
            [
                (InstancePathKind::Root, Some(data)),
                (InstancePathKind::Data, Some(data)),
                (InstancePathKind::Config, Some(data)),
                (InstancePathKind::ConfigHistory, Some(data)),
                (InstancePathKind::Temp, Some(data)),
                (InstancePathKind::Logs, Some(logs)),
                (InstancePathKind::Run, None),
            ]
        );
    }

    #[test]
    fn a_thousand_instances_across_three_locations_resolve_like_the_layout_and_never_nest() {
        // spec 0032, AC-10 per location.
        let layout = layout();
        let base = layout.roots().data().parent().unwrap().to_path_buf();
        let work_data = StorageLocationId::generate();
        let work_logs = StorageLocationId::generate();
        let disk_data = StorageLocationId::generate();
        let set = LocationSet::new(layout.roots().clone())
            .with_location(work_data, LocationKind::Data, base.join("work/Stackly"))
            .with_location(work_logs, LocationKind::Logs, base.join("work/Stackly"))
            .with_location(disk_data, LocationKind::Data, base.join("disk/Stackly"));
        let pairs = [
            (
                LocationKind::Data.built_in_id(),
                LocationKind::Logs.built_in_id(),
            ),
            (work_data, work_logs),
            (disk_data, LocationKind::Logs.built_in_id()),
        ];
        let mut seen: Vec<PathBuf> = Vec::new();
        for turn in 0..1000 {
            let id = InstanceId::generate();
            let (data, logs) = pairs[turn % pairs.len()];
            let expected =
                layout.instance_paths_in(id, &set.base(data).unwrap(), &set.base(logs).unwrap());
            let entries = instance_path_entries(id, data, logs);
            let resolved = |kind: InstancePathKind| {
                let entry = entries.iter().find(|entry| entry.kind() == kind).unwrap();
                set.resolve(entry).unwrap()
            };
            assert_eq!(resolved(InstancePathKind::Root), expected.root);
            assert_eq!(resolved(InstancePathKind::Data), expected.data);
            assert_eq!(resolved(InstancePathKind::Config), expected.config);
            assert_eq!(
                resolved(InstancePathKind::ConfigHistory),
                expected.config_history
            );
            assert_eq!(resolved(InstancePathKind::Temp), expected.temp);
            assert_eq!(resolved(InstancePathKind::Logs), expected.logs);
            assert_eq!(resolved(InstancePathKind::Run), expected.run);
            for path in [&expected.root, &expected.logs, &expected.run] {
                for other in &seen {
                    assert!(
                        !path.starts_with(other) && !other.starts_with(path),
                        "{} and {} nest",
                        path.display(),
                        other.display()
                    );
                }
            }
            seen.extend([expected.root, expected.logs, expected.run]);
        }
    }

    #[test]
    fn a_location_the_set_does_not_know_does_not_resolve() {
        let layout = layout();
        let set = LocationSet::new(layout.roots().clone());
        let unknown = StorageLocationId::generate();
        let id: InstanceId = ID.parse().unwrap();
        let entry = instance_path_entries(id, unknown, LocationKind::Logs.built_in_id())
            .into_iter()
            .find(|entry| entry.kind() == InstancePathKind::Data)
            .unwrap();

        assert_eq!(set.resolve(&entry), Err(UnknownLocation(unknown)));
        assert_eq!(set.kind(unknown), None);
        assert_eq!(
            set.kind(LocationKind::Cache.built_in_id()),
            Some(LocationKind::Cache)
        );
    }

    #[test]
    fn downloads_and_retained_folders_sit_under_their_location_folder() {
        let layout = layout();
        let folder = Path::new("/Volumes/Work/Stackly");
        let id: InstanceId = ID.parse().unwrap();

        assert_eq!(
            DirectoryLayout::downloads_dir_in(folder),
            folder.join("downloads")
        );
        assert_eq!(
            DirectoryLayout::downloads_dir_in(layout.roots().cache()),
            layout.downloads_dir()
        );
        let retained = layout.retained_instance_paths_in(id, folder, layout.roots().logs());
        assert_eq!(retained.root, folder.join("retained").join(ID));
        assert_eq!(retained.logs, layout.instance_log_dir(id));
    }

    #[test]
    fn instance_path_entries_of_one_id_are_the_documented_table() {
        let id: InstanceId = ID.parse().unwrap();
        let entries: Vec<_> = built_in_entries(id)
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
    fn the_agent_launch_record_is_a_file_in_the_data_root() {
        let layout = layout();

        assert_eq!(
            layout.agent_launch_record(),
            layout.roots().data().join("background-agent.json")
        );
    }

    #[test]
    fn the_diagnostics_staging_folder_is_in_the_cache_root() {
        let layout = layout();

        assert_eq!(
            layout.diagnostics_cache_dir(),
            layout.roots().cache().join("diagnostics")
        );
    }

    #[test]
    fn the_daemon_start_failure_record_is_a_file_in_the_data_root() {
        let layout = layout();

        assert_eq!(
            layout.daemon_start_failure_record(),
            layout.roots().data().join("daemon-start-failure.json")
        );
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
    fn an_imported_package_lives_under_provider_imports_by_its_digest() {
        let layout = layout();
        let digest: Sha256Digest = DIGEST.parse().unwrap();

        assert_eq!(
            layout.provider_import_dir(&digest),
            layout.roots().data().join("provider-imports").join(DIGEST)
        );
        assert!(
            !layout
                .standard_dirs()
                .contains(&layout.provider_imports_dir())
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
