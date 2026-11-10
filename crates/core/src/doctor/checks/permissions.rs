//! `permissions`: every root is a folder of this user that group and other
//! cannot write; every instance folder is a folder of this user with the
//! owner's write and search bits, and the first two levels of `data/` hold
//! no folder without them and nothing of another user. Symlinks are never
//! followed. In recovery mode only the roots are read.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use stackly_domain::doctor::{
    DoctorFindingCode, DoctorSeverity, PERMISSION_DEPTH_MAX, PERMISSION_ENTRIES_MAX,
    PERMISSION_FINDINGS_PER_INSTANCE,
};
use stackly_platform::{FileInfo, FileKind, RootKind, UserId};

use super::{CheckContext, CheckFailure, CheckFindings, stopped};
use crate::doctor::types::{Action, Finding, FindingFacts, InstanceRef, PathProblem, RootProblem};

/// Group or other may write.
const GROUP_OTHER_WRITE: u32 = 0o022;
/// The owner may write and search.
const OWNER_WRITE_SEARCH: u32 = 0o300;

const ROOTS: [RootKind; 4] = [
    RootKind::Data,
    RootKind::Logs,
    RootKind::Cache,
    RootKind::Runtime,
];

pub(super) fn run(
    context: &CheckContext,
    stop: &Arc<AtomicBool>,
) -> Result<CheckFindings, CheckFailure> {
    let uid = context.uid()?;
    let fs = context.sources.platform.fs();
    let mut findings = Vec::new();
    for kind in ROOTS {
        let path = context.sources.layout.canonical_root(kind);
        let info = fs
            .symlink_metadata(path)
            .map_err(|error| CheckFailure::io(error.to_string()))?;
        if let Some(problem) = root_problem(info.as_ref(), &uid) {
            findings.push(
                Finding::new(
                    DoctorFindingCode::RootNotPrivate,
                    DoctorSeverity::Error,
                    FindingFacts::Root {
                        root: kind,
                        path: path.to_path_buf(),
                        problem,
                        mode: info.as_ref().and_then(|info| info.mode),
                    },
                )
                .subject(kind.to_string()),
            );
        }
    }
    let mut omitted = Vec::new();
    let mut checked = 0usize;
    if context.supervisor().is_some() {
        for instance in context.instances()? {
            if stopped(stop) {
                break;
            }
            checked += 1;
            let reference = InstanceRef::of(&instance);
            let paths = context.sources.layout.instance_paths(instance.id());
            let mut walk = Walk {
                context,
                stop,
                uid: &uid,
                instance: &reference,
                entries: 0,
                found: Vec::new(),
            };
            for (label, folder) in [
                ("data", &paths.data),
                ("config", &paths.config),
                ("config-history", &paths.config_history),
                ("temp", &paths.temp),
            ] {
                walk.folder(label, folder);
            }
            walk.tree("data", &paths.data, 1);
            let mut found = walk.found;
            if found.len() > PERMISSION_FINDINGS_PER_INSTANCE {
                omitted.extend(
                    found
                        .drain(PERMISSION_FINDINGS_PER_INSTANCE..)
                        .map(|finding| finding.severity),
                );
            }
            findings.extend(found);
        }
    }
    let mut result = CheckFindings::checked(findings, checked);
    result.omitted = omitted;
    Ok(result)
}

fn root_problem(info: Option<&FileInfo>, uid: &UserId) -> Option<RootProblem> {
    let Some(info) = info else {
        return Some(RootProblem::Missing);
    };
    if info.kind != FileKind::Dir {
        return Some(RootProblem::NotDirectory);
    }
    if info.owner.as_ref().is_some_and(|owner| owner != uid) {
        return Some(RootProblem::ForeignOwner);
    }
    if info.mode.is_some_and(|mode| mode & GROUP_OTHER_WRITE != 0) {
        return Some(RootProblem::GroupOrOtherWritable);
    }
    None
}

/// The walk of one instance: at most [`PERMISSION_ENTRIES_MAX`] entries.
struct Walk<'a> {
    context: &'a CheckContext,
    stop: &'a AtomicBool,
    uid: &'a UserId,
    instance: &'a InstanceRef,
    entries: usize,
    found: Vec<Finding>,
}

impl Walk<'_> {
    /// Checks one of the four instance folders; a folder that is not there
    /// is the business of `instance_files`.
    fn folder(&mut self, label: &str, path: &Path) {
        let Ok(Some(info)) = self.context.sources.platform.fs().symlink_metadata(path) else {
            return;
        };
        if info.kind != FileKind::Dir {
            self.push(label.to_owned(), PathProblem::NotDirectory, &info);
        } else {
            self.entry(label.to_owned(), &info);
        }
    }

    /// Reads the entries of `path` (shown as `label`) at `depth`, without
    /// following symlinks.
    fn tree(&mut self, label: &str, path: &Path, depth: usize) {
        if depth > PERMISSION_DEPTH_MAX || stopped(self.stop) {
            return;
        }
        let Ok(entries) = self.context.sources.platform.fs().read_dir(path) else {
            return;
        };
        for entry in entries {
            if self.entries >= PERMISSION_ENTRIES_MAX || stopped(self.stop) {
                return;
            }
            self.entries += 1;
            let name = entry.name.to_string_lossy();
            let shown = format!("{label}/{name}");
            self.entry(shown.clone(), &entry.info);
            if entry.info.kind == FileKind::Dir {
                self.tree(&shown, &path.join(&entry.name), depth + 1);
            }
        }
    }

    fn entry(&mut self, shown: String, info: &FileInfo) {
        if info.owner.as_ref().is_some_and(|owner| owner != self.uid) {
            self.push(shown, PathProblem::ForeignOwner, info);
        } else if info.kind == FileKind::Dir
            && info
                .mode
                .is_some_and(|mode| mode & OWNER_WRITE_SEARCH != OWNER_WRITE_SEARCH)
        {
            self.push(shown, PathProblem::NotWritable, info);
        }
    }

    fn push(&mut self, path: String, problem: PathProblem, info: &FileInfo) {
        let code = match problem {
            PathProblem::ForeignOwner => DoctorFindingCode::PathForeignOwner,
            _ => DoctorFindingCode::PathNotWritable,
        };
        self.found.push(
            Finding::new(
                code,
                DoctorSeverity::Error,
                FindingFacts::Path {
                    path: path.clone(),
                    problem,
                    mode: info.mode,
                },
            )
            .about(self.instance.clone())
            .subject(path)
            .action(Action::ShowInstance(self.instance.id)),
        );
    }
}
