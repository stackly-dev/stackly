//! Reading a provider package folder into memory, through the `Filesystem`
//! port only.

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fmt;
use std::path::Path;

use stackly_platform::{FileKind, Filesystem, PlatformError};

use super::issue::{Issue, IssueCode, Issues, Location};
use super::names::is_template_name;

/// `provider.yaml`.
pub(crate) const MANIFEST_FILE: &str = "provider.yaml";
/// `versions.json`.
pub(crate) const VERSIONS_FILE: &str = "versions.json";
/// `schemas/settings.schema.json`.
pub(crate) const SETTINGS_SCHEMA_FILE: &str = "schemas/settings.schema.json";
/// The folder that holds the templates.
pub(crate) const TEMPLATES_DIR: &str = "templates";
/// The folder that holds the settings schema.
const SCHEMAS_DIR: &str = "schemas";

/// Largest `provider.yaml`, in bytes.
pub const MANIFEST_MAX_LEN: u64 = 256 * 1024;
/// Largest `versions.json`, in bytes.
pub const VERSIONS_MAX_LEN: u64 = 1024 * 1024;
/// Largest settings schema, in bytes.
pub const SETTINGS_SCHEMA_MAX_LEN: u64 = 128 * 1024;
/// Largest template, in bytes.
pub const TEMPLATE_MAX_LEN: u64 = 256 * 1024;
/// Most templates in one package.
pub const TEMPLATES_MAX: usize = 32;
/// Most bytes read for one package.
pub const PACKAGE_MAX_LEN: u64 = 4 * 1024 * 1024;

/// Entries the loader skips without reading.
const SKIPPED: [&str; 4] = [".DS_Store", "tests", "README.md", "LICENSE-NOTICES.md"];

/// The files of one provider package, in memory, keyed by their path
/// relative to the package folder (`provider.yaml`, `versions.json`,
/// `schemas/settings.schema.json`, and `templates/<name>`).
#[derive(Clone, PartialEq, Eq)]
pub struct PackageFiles {
    folder: String,
    files: BTreeMap<String, Vec<u8>>,
}

impl PackageFiles {
    /// Bundles files with the name of the folder they came from.
    #[must_use]
    pub fn new(folder: impl Into<String>, files: BTreeMap<String, Vec<u8>>) -> Self {
        Self {
            folder: folder.into(),
            files,
        }
    }

    /// Returns the package folder's name.
    #[must_use]
    pub fn folder(&self) -> &str {
        &self.folder
    }

    /// Returns the bytes of one file, by its relative path.
    #[must_use]
    pub fn get(&self, path: &str) -> Option<&[u8]> {
        self.files.get(path).map(Vec::as_slice)
    }

    /// Returns every file, in path order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &[u8])> {
        self.files
            .iter()
            .map(|(path, bytes)| (path.as_str(), bytes.as_slice()))
    }
}

impl fmt::Debug for PackageFiles {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sizes: BTreeMap<&str, usize> = self
            .files
            .iter()
            .map(|(path, bytes)| (path.as_str(), bytes.len()))
            .collect();
        f.debug_struct("PackageFiles")
            .field("folder", &self.folder)
            .field("sizes", &sizes)
            .finish()
    }
}

/// How [`ValidatedPackage::from_files`](super::ValidatedPackage::from_files)
/// checks the package's identity against its folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum NameCheck {
    /// The name part of `metadata.id` must equal the folder name. For
    /// packages in the repo and the built in catalog.
    FolderName,
    /// No folder check, for a pinned copy whose identity the caller checks
    /// through its digest.
    Skip,
}

/// Error returned when a provider package cannot be read or is invalid.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("provider package {package} has {} issues", issues.len())]
#[non_exhaustive]
pub struct PackageError {
    /// The package folder's name.
    pub package: String,
    /// Every issue found, at most [`MAX_ISSUES`](super::MAX_ISSUES) plus a
    /// final `too_many_issues`.
    pub issues: Vec<Issue>,
}

/// Reads a provider package folder into memory.
///
/// Only [`Filesystem::symlink_metadata`], [`Filesystem::read_dir`], and
/// [`Filesystem::read_file_limited`] are called, so reading never writes,
/// runs, or resolves anything. The folder must be a folder, not a symlink;
/// symlinks, FIFOs, devices, and sockets anywhere are rejected, as are
/// entries the package layout does not allow. `.DS_Store` is skipped, and
/// `tests/`, `README.md`, and `LICENSE-NOTICES.md` are never read.
///
/// # Errors
///
/// Returns a [`PackageError`] with every issue found: a missing file,
/// an unexpected entry, a symlink or special file, a file above its size
/// limit, more than [`TEMPLATES_MAX`] templates, more than
/// [`PACKAGE_MAX_LEN`] bytes in total, a file that is not UTF-8 or starts
/// with a byte order mark, or a read that failed.
pub fn read_package(fs: &dyn Filesystem, dir: &Path) -> Result<PackageFiles, PackageError> {
    let folder = dir
        .file_name()
        .and_then(OsStr::to_str)
        .unwrap_or_default()
        .to_owned();
    let mut reader = Reader {
        fs,
        issues: Issues::new(),
        files: BTreeMap::new(),
        total: 0,
    };
    reader.read_root(dir);
    let Reader { issues, files, .. } = reader;
    if issues.is_empty() {
        Ok(PackageFiles::new(folder, files))
    } else {
        Err(PackageError {
            package: folder,
            issues: issues.into_vec(),
        })
    }
}

struct Reader<'a> {
    fs: &'a dyn Filesystem,
    issues: Issues,
    files: BTreeMap<String, Vec<u8>>,
    total: u64,
}

impl Reader<'_> {
    fn issue(&mut self, file: &str, code: IssueCode, message: impl Into<String>) {
        self.issues
            .push(Issue::new(file, Location::File, code, message));
    }

    fn read_root(&mut self, dir: &Path) {
        match self.fs.symlink_metadata(dir) {
            Ok(Some(info)) if info.kind == FileKind::Dir => {}
            Ok(Some(info)) if info.kind == FileKind::Symlink => {
                return self.issue(
                    "",
                    IssueCode::SymlinkNotAllowed,
                    "the package folder is a symlink",
                );
            }
            Ok(Some(_)) => {
                return self.issue(
                    "",
                    IssueCode::NotARegularFile,
                    "the package path is not a folder",
                );
            }
            Ok(None) => {
                return self.issue(
                    "",
                    IssueCode::MissingFile,
                    "the package folder does not exist",
                );
            }
            Err(err) => return self.read_failed("", &err),
        }
        let Some(entries) = self.list(dir, "") else {
            return;
        };
        let (mut manifest, mut versions, mut schemas) = (false, false, false);
        for (name, kind) in entries {
            match name.as_str() {
                MANIFEST_FILE => {
                    manifest = true;
                    self.read_file(dir, &name, kind, MANIFEST_MAX_LEN);
                }
                VERSIONS_FILE => {
                    versions = true;
                    self.read_file(dir, &name, kind, VERSIONS_MAX_LEN);
                }
                SCHEMAS_DIR if kind == FileKind::Dir => {
                    schemas = true;
                    self.read_schemas(&dir.join(SCHEMAS_DIR));
                }
                TEMPLATES_DIR if kind == FileKind::Dir => {
                    self.read_templates(&dir.join(TEMPLATES_DIR))
                }
                _ if SKIPPED.contains(&name.as_str()) => {}
                _ => self.issue(
                    &name,
                    IssueCode::UnexpectedEntry,
                    "entry not allowed in a provider package",
                ),
            }
        }
        for (present, file) in [
            (manifest, MANIFEST_FILE),
            (versions, VERSIONS_FILE),
            (schemas, SETTINGS_SCHEMA_FILE),
        ] {
            if !present {
                self.issue(file, IssueCode::MissingFile, "required file is missing");
            }
        }
    }

    fn read_schemas(&mut self, dir: &Path) {
        let Some(entries) = self.list(dir, SCHEMAS_DIR) else {
            return;
        };
        let mut found = false;
        for (name, kind) in entries {
            let path = format!("{SCHEMAS_DIR}/{name}");
            if name == "settings.schema.json" {
                found = true;
                self.read_file(dir, &path, kind, SETTINGS_SCHEMA_MAX_LEN);
            } else if name != ".DS_Store" {
                self.issue(
                    &path,
                    IssueCode::UnexpectedEntry,
                    "schemas/ holds only settings.schema.json",
                );
            }
        }
        if !found {
            self.issue(
                SETTINGS_SCHEMA_FILE,
                IssueCode::MissingFile,
                "required file is missing",
            );
        }
    }

    fn read_templates(&mut self, dir: &Path) {
        let Some(entries) = self.list(dir, TEMPLATES_DIR) else {
            return;
        };
        let mut count = 0;
        for (name, kind) in entries {
            let path = format!("{TEMPLATES_DIR}/{name}");
            if name == ".DS_Store" {
                continue;
            }
            if !is_template_name(&name) {
                self.issue(
                    &path,
                    IssueCode::UnexpectedEntry,
                    "templates must be named like name.tmpl: lowercase letters, digits, and . _ -",
                );
                continue;
            }
            count += 1;
            if count > TEMPLATES_MAX {
                self.issue(
                    &path,
                    IssueCode::FileTooLarge,
                    "a package holds at most 32 templates",
                );
                continue;
            }
            self.read_file(dir, &path, kind, TEMPLATE_MAX_LEN);
        }
    }

    /// Lists a folder as (name, kind) pairs in name order, rejecting
    /// symlinks and names that are not UTF-8.
    fn list(&mut self, dir: &Path, prefix: &str) -> Option<Vec<(String, FileKind)>> {
        let entries = match self.fs.read_dir(dir) {
            Ok(entries) => entries,
            Err(err) => {
                self.read_failed(prefix, &err);
                return None;
            }
        };
        let mut list = Vec::new();
        for entry in entries {
            let Some(name) = entry.name.to_str() else {
                let shown = entry.name.to_string_lossy().into_owned();
                self.issue(
                    &join(prefix, &shown),
                    IssueCode::NotUtf8,
                    "entry name is not UTF-8",
                );
                continue;
            };
            if entry.info.kind == FileKind::Symlink {
                self.issue(
                    &join(prefix, name),
                    IssueCode::SymlinkNotAllowed,
                    "symlinks are not allowed in a provider package",
                );
                continue;
            }
            list.push((name.to_owned(), entry.info.kind));
        }
        list.sort_by(|a, b| a.0.cmp(&b.0));
        Some(list)
    }

    fn read_file(&mut self, dir: &Path, path: &str, kind: FileKind, max_len: u64) {
        if kind != FileKind::File {
            return self.issue(path, IssueCode::NotARegularFile, "not a regular file");
        }
        let name = path.rsplit('/').next().unwrap_or(path);
        let budget = PACKAGE_MAX_LEN.saturating_sub(self.total);
        match self
            .fs
            .read_file_limited(&dir.join(name), max_len.min(budget))
        {
            Ok(bytes) => {
                self.total += bytes.len() as u64;
                self.files.insert(path.to_owned(), bytes);
            }
            Err(PlatformError::FileTooLarge { .. }) if max_len > budget => self.issue(
                path,
                IssueCode::FileTooLarge,
                "the package holds more than 4 MiB",
            ),
            Err(PlatformError::FileTooLarge { .. }) => self.issue(
                path,
                IssueCode::FileTooLarge,
                format!("file is larger than {max_len} bytes"),
            ),
            Err(PlatformError::NotARegularFile { .. }) => {
                self.issue(path, IssueCode::NotARegularFile, "not a regular file");
            }
            Err(err) => self.read_failed(path, &err),
        }
    }

    fn read_failed(&mut self, path: &str, err: &PlatformError) {
        self.issue(
            path,
            IssueCode::NotARegularFile,
            format!("cannot read: {err}"),
        );
    }
}

fn join(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        name.to_owned()
    } else {
        format!("{prefix}/{name}")
    }
}

/// Checks the file set of [`PackageFiles`] and decodes each file as UTF-8,
/// for `from_files`: required files present, only allowed paths, template
/// names and count, size limits, and UTF-8 without a byte order mark.
pub(crate) fn check_files<'a>(
    files: &'a PackageFiles,
    issues: &mut Issues,
) -> BTreeMap<&'a str, &'a str> {
    let mut texts = BTreeMap::new();
    let mut templates = 0;
    let mut total: u64 = 0;
    for (path, bytes) in files.iter() {
        let limit = match path {
            MANIFEST_FILE => MANIFEST_MAX_LEN,
            VERSIONS_FILE => VERSIONS_MAX_LEN,
            SETTINGS_SCHEMA_FILE => SETTINGS_SCHEMA_MAX_LEN,
            _ => match path.strip_prefix("templates/") {
                Some(name) if is_template_name(name) => {
                    templates += 1;
                    if templates > TEMPLATES_MAX {
                        issues.push(Issue::new(
                            path,
                            Location::File,
                            IssueCode::FileTooLarge,
                            "a package holds at most 32 templates",
                        ));
                        continue;
                    }
                    TEMPLATE_MAX_LEN
                }
                _ => {
                    issues.push(Issue::new(
                        path,
                        Location::File,
                        IssueCode::UnexpectedEntry,
                        "entry not allowed in a provider package",
                    ));
                    continue;
                }
            },
        };
        total = total.saturating_add(bytes.len() as u64);
        if bytes.len() as u64 > limit {
            issues.push(Issue::new(
                path,
                Location::File,
                IssueCode::FileTooLarge,
                format!("file is larger than {limit} bytes"),
            ));
            continue;
        }
        if bytes.starts_with(&[0xef, 0xbb, 0xbf]) {
            issues.push(Issue::new(
                path,
                Location::File,
                IssueCode::NotUtf8,
                "file starts with a byte order mark",
            ));
            continue;
        }
        match std::str::from_utf8(bytes) {
            Ok(text) => {
                texts.insert(path, text);
            }
            Err(_) => issues.push(Issue::new(
                path,
                Location::File,
                IssueCode::NotUtf8,
                "file is not UTF-8",
            )),
        }
    }
    if total > PACKAGE_MAX_LEN {
        issues.push(Issue::new(
            "",
            Location::File,
            IssueCode::FileTooLarge,
            "the package holds more than 4 MiB",
        ));
    }
    for required in [MANIFEST_FILE, VERSIONS_FILE, SETTINGS_SCHEMA_FILE] {
        if files.get(required).is_none() {
            issues.push(Issue::new(
                required,
                Location::File,
                IssueCode::MissingFile,
                "required file is missing",
            ));
        }
    }
    texts
}
