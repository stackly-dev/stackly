//! The static binary checks of an extracted tree (spec 0018, *Validasi
//! binary* steps 3 and 4, moved here from the spike audit of spec 0003):
//! every executable has an arm64 slice and runs on the host macOS, and
//! every library a Mach-O file loads is a system library or a file inside
//! the tree, with the non system ones exactly the artifact's `bundled` and
//! `own` dependencies.
//!
//! Resolution works on the entries the extractor reported, never on the
//! filesystem: `@executable_path` is the folder of each main executable
//! (`MH_EXECUTE`) in the tree, `@loader_path` the folder of the loading
//! file, and `@rpath` tries the `LC_RPATH` entries of the loading file and
//! then of every main executable, in order. Symlinks resolve as the
//! extractor checked them, at most [`SYMLINK_CHAIN_MAX`] of them per path.
//! Install names under `/usr/lib/` and `/System/Library/` pass without an
//! existence check, since they live in the dyld shared cache.

use std::collections::{BTreeSet, HashMap};

use stackly_domain::OsVersion;
use stackly_platform::EntryPath;

use super::archive::limits::SYMLINK_CHAIN_MAX;
use super::archive::{EntryKind, EntryRecord};
use super::macho::{LibraryKind, MachO, Slice};

/// Install name prefixes of system libraries.
pub const SYSTEM_PREFIXES: [&str; 2] = ["/usr/lib/", "/System/Library/"];

/// One Mach-O file of the tree, parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct MachOEntry {
    /// Where it lies in the tree.
    pub path: EntryPath,
    /// What its headers say.
    pub macho: MachO,
}

impl MachOEntry {
    /// Pairs a path with its parsed headers.
    #[must_use]
    pub fn new(path: EntryPath, macho: MachO) -> Self {
        Self { path, macho }
    }
}

/// What the checks look at.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct ClosureInput<'a> {
    /// Every entry of the tree, from the extract report.
    pub entries: &'a [EntryRecord],
    /// Every Mach-O file of the tree.
    pub machos: &'a [MachOEntry],
    /// The artifact's executables.
    pub executables: &'a [EntryPath],
    /// The macOS version of this Mac.
    pub host_os: &'a OsVersion,
    /// True for a maintainer artifact (`build` present), where an absolute
    /// `LC_RPATH` outside the system fails instead of warning.
    pub maintainer: bool,
    /// The file names of the artifact's `bundled` and `own` dependencies,
    /// or `None` when the catalog lists no dependencies, which skips the
    /// comparison with a warning.
    pub expected_libraries: Option<&'a [String]>,
}

impl<'a> ClosureInput<'a> {
    /// Bundles the inputs.
    #[must_use]
    pub fn new(
        entries: &'a [EntryRecord],
        machos: &'a [MachOEntry],
        executables: &'a [EntryPath],
        host_os: &'a OsVersion,
        maintainer: bool,
        expected_libraries: Option<&'a [String]>,
    ) -> Self {
        Self {
            entries,
            machos,
            executables,
            host_os,
            maintainer,
            expected_libraries,
        }
    }
}

/// What passed checks found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct ClosureReport {
    /// Findings that do not fail the install, sorted.
    pub warnings: Vec<String>,
    /// The file names of the non system libraries found inside the tree,
    /// sorted.
    pub libraries: Vec<String>,
}

/// Why the checks failed, one variant per error code of spec 0018.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ClosureError {
    /// An executable is not Mach-O or has no arm64 slice
    /// (`runtime.architecture_mismatch`).
    #[error("{path} has no arm64 slice (found {arches:?})")]
    ArchitectureMismatch {
        /// The executable.
        path: String,
        /// The architectures it has.
        arches: Vec<String>,
    },
    /// An executable needs a newer macOS than this Mac runs
    /// (`runtime.os_too_old`).
    #[error("{path} needs macOS {required}, this Mac runs {host}")]
    OsTooOld {
        /// The executable.
        path: String,
        /// The macOS it needs.
        required: OsVersion,
        /// The macOS this Mac runs.
        host: OsVersion,
    },
    /// A library resolves outside the tree and the system, or not at all,
    /// or a maintainer artifact carries an absolute `LC_RPATH` outside the
    /// system (`runtime.dependency_outside_runtime`).
    #[error("{path} loads {library} from outside the runtime")]
    DependencyOutsideRuntime {
        /// The Mach-O file that loads it.
        path: String,
        /// The install name or `LC_RPATH`.
        library: String,
    },
    /// The non system libraries found differ from the artifact's
    /// dependencies (`runtime.dependency_mismatch`).
    #[error(
        "libraries differ from the dependencies: missing {missing:?}, unexpected {unexpected:?}"
    )]
    DependencyMismatch {
        /// Dependencies listed but not found.
        missing: Vec<String>,
        /// Libraries found but not listed.
        unexpected: Vec<String>,
    },
}

/// Runs the architecture, macOS minimum, closure, and dependency checks.
///
/// # Errors
///
/// Returns the first [`ClosureError`] found, in that order of checks.
pub fn check(input: &ClosureInput<'_>) -> Result<ClosureReport, ClosureError> {
    let tree = Tree::new(input.entries);
    let mut report = ClosureReport::default();
    check_executables(input, &mut report)?;
    let mains: Vec<(&EntryPath, &Slice)> = input
        .machos
        .iter()
        .filter_map(|entry| Some((&entry.path, entry.macho.arm64.as_ref()?)))
        .filter(|(_, slice)| slice.is_executable())
        .collect();
    let mut found = BTreeSet::new();
    for entry in input.machos {
        let Some(slice) = &entry.macho.arm64 else {
            continue;
        };
        check_rpaths(input, entry, slice, &mut report)?;
        for library in &slice.libraries {
            if slice.install_name.as_deref() == Some(library.name.as_str())
                || is_system(&library.name)
            {
                continue;
            }
            match resolve(&tree, &entry.path, slice, &library.name, &mains) {
                Resolved::Inside => {
                    let file_name = library.name.rsplit('/').next().unwrap_or(&library.name);
                    found.insert(file_name.to_owned());
                }
                Resolved::System => {}
                Resolved::Missing if library.kind == LibraryKind::Weak => {
                    report.warnings.push(format!(
                        "{}: weak library {} is missing",
                        entry.path, library.name
                    ));
                }
                Resolved::Outside | Resolved::Missing => {
                    return Err(ClosureError::DependencyOutsideRuntime {
                        path: entry.path.to_string(),
                        library: library.name.clone(),
                    });
                }
            }
        }
    }
    match input.expected_libraries {
        Some(expected) => {
            let expected: BTreeSet<String> = expected.iter().cloned().collect();
            let missing: Vec<String> = expected.difference(&found).cloned().collect();
            let unexpected: Vec<String> = found.difference(&expected).cloned().collect();
            if !missing.is_empty() || !unexpected.is_empty() {
                return Err(ClosureError::DependencyMismatch {
                    missing,
                    unexpected,
                });
            }
        }
        None => report
            .warnings
            .push("dependencies not declared; the library set was not compared".to_owned()),
    }
    report.libraries = found.into_iter().collect();
    report.warnings.sort();
    report.warnings.dedup();
    Ok(report)
}

/// Every executable is Mach-O with an arm64 slice that runs on the host;
/// another Mach-O file without one only warns, since an arm64 process
/// never loads it.
fn check_executables(
    input: &ClosureInput<'_>,
    report: &mut ClosureReport,
) -> Result<(), ClosureError> {
    for executable in input.executables {
        let parsed = input
            .machos
            .iter()
            .find(|entry| entry.path == *executable)
            .map(|entry| &entry.macho);
        let Some(slice) = parsed.and_then(|macho| macho.arm64.as_ref()) else {
            return Err(ClosureError::ArchitectureMismatch {
                path: executable.to_string(),
                arches: parsed
                    .map(|macho| macho.arches.iter().map(ToString::to_string).collect())
                    .unwrap_or_default(),
            });
        };
        if let Some(required) = &slice.min_os
            && required > input.host_os
        {
            return Err(ClosureError::OsTooOld {
                path: executable.to_string(),
                required: *required,
                host: *input.host_os,
            });
        }
    }
    for entry in input.machos {
        if entry.macho.arm64.is_none() && !input.executables.contains(&entry.path) {
            let arches: Vec<String> = entry.macho.arches.iter().map(ToString::to_string).collect();
            report.warnings.push(format!(
                "{}: no arm64 slice ({}), never loaded by an arm64 process",
                entry.path,
                arches.join(", ")
            ));
        }
    }
    Ok(())
}

/// An absolute `LC_RPATH` outside the system fails a maintainer artifact
/// and warns for an upstream one.
fn check_rpaths(
    input: &ClosureInput<'_>,
    entry: &MachOEntry,
    slice: &Slice,
    report: &mut ClosureReport,
) -> Result<(), ClosureError> {
    for rpath in &slice.rpaths {
        if rpath.starts_with('/') && !is_system(&format!("{}/", rpath.trim_end_matches('/'))) {
            if input.maintainer {
                return Err(ClosureError::DependencyOutsideRuntime {
                    path: entry.path.to_string(),
                    library: rpath.clone(),
                });
            }
            report.warnings.push(format!(
                "{}: LC_RPATH {rpath} is absolute outside the system",
                entry.path
            ));
        }
    }
    Ok(())
}

/// True for a path under a system prefix with no `..` component.
fn is_system(path: &str) -> bool {
    SYSTEM_PREFIXES
        .iter()
        .any(|prefix| path.starts_with(prefix))
        && !path.split('/').any(|part| part == "..")
}

/// Where one library reference ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Resolved {
    /// At a regular file inside the tree.
    Inside,
    /// At a system path.
    System,
    /// Only at absolute paths outside the tree and the system.
    Outside,
    /// Nowhere.
    Missing,
}

/// Resolves one reference the way dyld would: the loading file's own
/// rpaths first, then those of every main executable, with the paths
/// inside the tree checked before the system ones.
fn resolve(
    tree: &Tree<'_>,
    loader: &EntryPath,
    slice: &Slice,
    name: &str,
    mains: &[(&EntryPath, &Slice)],
) -> Resolved {
    let loader_dir = parent_of(loader);
    let exe_dirs: Vec<Vec<&str>> = if slice.is_executable() {
        vec![loader_dir.clone()]
    } else {
        mains.iter().map(|(path, _)| parent_of(path)).collect()
    };
    let mut candidates: Vec<Candidate> = Vec::new();
    if let Some(rest) = name.strip_prefix("@rpath/") {
        for rpath in &slice.rpaths {
            candidates.extend(expand(rpath, &loader_dir, &exe_dirs, rest));
        }
        for (path, main) in mains {
            let main_dir = parent_of(path);
            for rpath in &main.rpaths {
                candidates.extend(expand(
                    rpath,
                    &main_dir,
                    std::slice::from_ref(&main_dir),
                    rest,
                ));
            }
        }
    } else if name.starts_with('@') {
        candidates.extend(expand(name, &loader_dir, &exe_dirs, ""));
    } else {
        candidates.push(Candidate::Absolute(name.to_owned()));
    }
    // The tree is tried first: a system candidate passes without an
    // existence check, so it must not shadow a library the tree bundles.
    let in_tree = candidates.iter().any(|candidate| match candidate {
        Candidate::Tree(dir, rest) => tree.is_file(dir, rest),
        Candidate::Absolute(_) => false,
    });
    let absolute = || {
        candidates.iter().filter_map(|candidate| match candidate {
            Candidate::Absolute(path) => Some(path),
            Candidate::Tree(..) => None,
        })
    };
    if in_tree {
        Resolved::Inside
    } else if absolute().any(|path| is_system(path)) {
        Resolved::System
    } else if absolute().any(|path| path.starts_with('/')) {
        Resolved::Outside
    } else {
        Resolved::Missing
    }
}

/// A place a reference may resolve to.
enum Candidate<'a> {
    /// A folder of the tree and the path below it.
    Tree(Vec<&'a str>, String),
    /// An absolute path outside the tree.
    Absolute(String),
}

/// Expands `@loader_path` and `@executable_path` in `path`, joined with
/// `rest`; `@executable_path` gives one candidate per main executable.
fn expand<'a>(
    path: &str,
    loader_dir: &[&'a str],
    exe_dirs: &[Vec<&'a str>],
    rest: &str,
) -> Vec<Candidate<'a>> {
    let join = |tail: &str| {
        [tail.trim_start_matches('/'), rest]
            .iter()
            .filter(|part| !part.is_empty())
            .copied()
            .collect::<Vec<_>>()
            .join("/")
    };
    if let Some(tail) = path.strip_prefix("@loader_path") {
        vec![Candidate::Tree(loader_dir.to_vec(), join(tail))]
    } else if let Some(tail) = path.strip_prefix("@executable_path") {
        exe_dirs
            .iter()
            .map(|dir| Candidate::Tree(dir.clone(), join(tail)))
            .collect()
    } else if path.starts_with('/') {
        let base = path.trim_end_matches('/');
        vec![Candidate::Absolute(if rest.is_empty() {
            base.to_owned()
        } else {
            format!("{base}/{rest}")
        })]
    } else {
        // A relative rpath resolves against the working directory, which
        // is never inside the runtime.
        Vec::new()
    }
}

/// The folder of `path`, as components.
fn parent_of(path: &EntryPath) -> Vec<&str> {
    let mut components: Vec<&str> = path.components().collect();
    components.pop();
    components
}

/// The extracted entries, looked up by path.
struct Tree<'a> {
    entries: HashMap<&'a str, &'a EntryRecord>,
}

impl<'a> Tree<'a> {
    fn new(entries: &'a [EntryRecord]) -> Self {
        Self {
            entries: entries
                .iter()
                .map(|entry| (entry.path.as_str(), entry))
                .collect(),
        }
    }

    /// True when `rest`, from the folder `dir`, ends at a regular file
    /// inside the tree, following symlinks in any component.
    fn is_file(&self, dir: &[&str], rest: &str) -> bool {
        let mut stack: Vec<String> = dir.iter().map(|part| (*part).to_owned()).collect();
        let mut pending: Vec<String> = rest.rsplit('/').map(str::to_owned).collect();
        let mut hops = 0;
        while let Some(component) = pending.pop() {
            match component.as_str() {
                "" | "." => continue,
                ".." => {
                    if stack.pop().is_none() {
                        return false;
                    }
                    continue;
                }
                _ => {}
            }
            stack.push(component);
            let path = stack.join("/");
            let last = pending.iter().all(|part| part.is_empty() || part == ".");
            match self
                .entries
                .get(path.as_str())
                .map(|entry| (entry.kind, &entry.target))
            {
                Some((EntryKind::Dir, _)) if !last => {}
                Some((EntryKind::File, _)) if last => return true,
                Some((EntryKind::Symlink, Some(target))) => {
                    hops += 1;
                    if hops > SYMLINK_CHAIN_MAX || target.starts_with('/') {
                        return false;
                    }
                    stack.pop();
                    pending.extend(target.rsplit('/').map(str::to_owned));
                }
                _ => return false,
            }
        }
        false
    }
}

#[cfg(test)]
mod tests;
