//! Test catalogs (feature `test-support`, spec 0017): [`CatalogBuilder`]
//! writes a valid catalog folder through the `Filesystem` port with the
//! same index, digest, and rules as `cargo xtask catalog build`, and
//! [`CatalogFixture`] holds the packages the status and gate tests use.
//!
//! `stackly-testkit` may not depend on this crate (spec 0001), so the
//! builder lives here; `stacklyd` and `stackly-cli` use it as a dev
//! dependency.

use std::collections::BTreeMap;
use std::path::{Component, Path};

use jiff::Timestamp;
use stackly_platform::{Filesystem, PlatformError};

use crate::provider::{NameCheck, PackageError, PackageFiles, ValidatedPackage};

use super::assemble::{
    AssembleOptions, AssembledCatalog, SourceNote, SourcePlanned, SourceProvider, SourceSpec,
    assemble,
};
use super::index::{CATALOG_INDEX_FILE, CatalogIndex, package_dir};

macro_rules! fixture_file {
    ($name:literal, $path:literal) => {
        (
            $path,
            include_bytes!(concat!("fixtures/", $name, "/", $path)).as_slice(),
        )
    };
}

/// A package the catalog tests use: a copy of the NATS package with
/// another ID and other artifact targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CatalogFixture {
    /// `official/linuxonly`: one `available` artifact for `linux-x86_64`.
    LinuxOnly,
    /// `official/intelonly`: one `available` artifact for
    /// `macos-x86_64`.
    IntelOnly,
    /// `official/blockedhost`: one `blocked` artifact for
    /// `macos-aarch64`.
    BlockedHost,
}

impl CatalogFixture {
    /// Every fixture.
    pub const ALL: [Self; 3] = [Self::LinuxOnly, Self::IntelOnly, Self::BlockedHost];

    /// Returns its provider ID.
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::LinuxOnly => "official/linuxonly",
            Self::IntelOnly => "official/intelonly",
            Self::BlockedHost => "official/blockedhost",
        }
    }

    /// Returns its package files, the way `read_package` returns them.
    #[must_use]
    pub fn files(self) -> PackageFiles {
        let (folder, files): (&str, [(&str, &[u8]); 4]) = match self {
            Self::LinuxOnly => (
                "linuxonly",
                [
                    fixture_file!("linuxonly", "provider.yaml"),
                    fixture_file!("linuxonly", "versions.json"),
                    fixture_file!("linuxonly", "schemas/settings.schema.json"),
                    fixture_file!("linuxonly", "templates/nats.conf.tmpl"),
                ],
            ),
            Self::IntelOnly => (
                "intelonly",
                [
                    fixture_file!("intelonly", "provider.yaml"),
                    fixture_file!("intelonly", "versions.json"),
                    fixture_file!("intelonly", "schemas/settings.schema.json"),
                    fixture_file!("intelonly", "templates/nats.conf.tmpl"),
                ],
            ),
            Self::BlockedHost => (
                "blockedhost",
                [
                    fixture_file!("blockedhost", "provider.yaml"),
                    fixture_file!("blockedhost", "versions.json"),
                    fixture_file!("blockedhost", "schemas/settings.schema.json"),
                    fixture_file!("blockedhost", "templates/nats.conf.tmpl"),
                ],
            ),
        };
        PackageFiles::new(
            folder,
            files
                .into_iter()
                .map(|(path, bytes)| (path.to_owned(), bytes.to_vec()))
                .collect(),
        )
    }

    /// Returns its validated package.
    ///
    /// # Panics
    ///
    /// Panics when a fixture no longer validates, which is a bug in the
    /// fixture files.
    #[must_use]
    pub fn package(self) -> ValidatedPackage {
        ValidatedPackage::from_files(self.files(), NameCheck::FolderName)
            .expect("the catalog fixture packages validate")
    }
}

/// Why a test catalog could not be built or written.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum CatalogBuildError {
    /// A package did not validate.
    #[error("package {id} is invalid: {issues:?}")]
    Package {
        /// The provider ID.
        id: String,
        /// Its issues, as text.
        issues: Vec<String>,
    },
    /// The catalog broke a build rule.
    #[error("catalog rules failed: {0:?}")]
    Rules(Vec<String>),
    /// Writing a file failed.
    #[error("cannot write the catalog: {0}")]
    Write(String),
}

/// Builds a test catalog folder. Defaults: revision 1, published at
/// `2026-10-01T00:00:00Z`, no entries, `testing` packages refused.
#[derive(Debug, Clone)]
pub struct CatalogBuilder {
    revision: u64,
    published_at: Timestamp,
    allow_testing: bool,
    providers: Vec<(SourceProvider, PackageFiles)>,
    planned: Vec<SourcePlanned>,
}

impl Default for CatalogBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl CatalogBuilder {
    /// Starts an empty catalog.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            revision: 1,
            published_at: Timestamp::constant(1_790_812_800, 0),
            allow_testing: false,
            providers: Vec::new(),
            planned: Vec::new(),
        }
    }

    /// Sets the catalog revision.
    #[must_use]
    pub const fn revision(mut self, revision: u64) -> Self {
        self.revision = revision;
        self
    }

    /// Sets when the catalog was published.
    #[must_use]
    pub const fn published_at(mut self, published_at: Timestamp) -> Self {
        self.published_at = published_at;
        self
    }

    /// Accepts packages of the `testing` namespace, such as `providers/fake`.
    #[must_use]
    pub const fn allow_testing(mut self) -> Self {
        self.allow_testing = true;
        self
    }

    /// Adds a provider entry with its package files, such as a repo package
    /// read with `read_package`.
    #[must_use]
    pub fn package(mut self, id: &str, files: PackageFiles) -> Self {
        self.providers.push((
            SourceProvider {
                id: id.to_owned(),
                mark: None,
                tile: None,
                platform_notes: Vec::new(),
            },
            files,
        ));
        self
    }

    /// Adds a fixture package as a provider entry.
    #[must_use]
    pub fn fixture(self, fixture: CatalogFixture) -> Self {
        self.package(fixture.id(), fixture.files())
    }

    /// Adds a platform note to the provider entry `id` added before.
    #[must_use]
    pub fn platform_note(mut self, id: &str, os: &str, status: &str, note: &str) -> Self {
        if let Some((provider, _)) = self.providers.iter_mut().find(|(p, _)| p.id == id) {
            provider.platform_notes.push(SourceNote {
                os: os.to_owned(),
                status: status.to_owned(),
                note: note.to_owned(),
            });
        }
        self
    }

    /// Adds a planned entry.
    #[must_use]
    pub fn planned(
        mut self,
        id: &str,
        display_name: &str,
        category: &str,
        description: &str,
    ) -> Self {
        self.planned.push(SourcePlanned {
            id: id.to_owned(),
            display_name: display_name.to_owned(),
            category: category.to_owned(),
            description: description.to_owned(),
            note: None,
            mark: None,
            tile: None,
        });
        self
    }

    /// Validates the packages and assembles the catalog with the build
    /// rules (no `minOs` ceiling and no clock).
    ///
    /// # Errors
    ///
    /// Returns [`CatalogBuildError::Package`] for a package that does not
    /// validate and [`CatalogBuildError::Rules`] for a broken build rule.
    pub fn assemble(&self) -> Result<AssembledCatalog, CatalogBuildError> {
        let mut packages = BTreeMap::new();
        for (provider, files) in &self.providers {
            let package = ValidatedPackage::from_files(files.clone(), NameCheck::FolderName)
                .map_err(|err: PackageError| CatalogBuildError::Package {
                    id: provider.id.clone(),
                    issues: err.issues.iter().map(ToString::to_string).collect(),
                })?;
            packages.insert(provider.id.clone(), package);
        }
        let spec = SourceSpec {
            revision: self.revision,
            published_at: self.published_at,
            providers: self.providers.iter().map(|(p, _)| p.clone()).collect(),
            planned: self.planned.clone(),
        };
        assemble(
            &spec,
            &packages,
            AssembleOptions {
                allow_testing: self.allow_testing,
                ..AssembleOptions::default()
            },
        )
        .map_err(CatalogBuildError::Rules)
    }

    /// Writes the catalog folder `dir` through `fs`: `catalog.json` and
    /// `packages/<id>/` copies, creating `dir` (whose parent must exist) and
    /// the package folders with mode 0700.
    ///
    /// # Errors
    ///
    /// Returns the [`assemble`](Self::assemble) errors, and
    /// [`CatalogBuildError::Write`] when a folder or file cannot be
    /// written.
    pub fn write(
        &self,
        fs: &dyn Filesystem,
        dir: &Path,
    ) -> Result<CatalogIndex, CatalogBuildError> {
        let catalog = self.assemble()?;
        write_folder(fs, dir, &catalog).map_err(|err| CatalogBuildError::Write(err.to_string()))?;
        Ok(catalog.index)
    }
}

fn write_folder(
    fs: &dyn Filesystem,
    dir: &Path,
    catalog: &AssembledCatalog,
) -> Result<(), PlatformError> {
    create_if_missing(fs, dir)?;
    fs.write_file_atomic(
        &dir.join(CATALOG_INDEX_FILE),
        catalog.index.to_pretty_json().as_bytes(),
    )?;
    for (id, files) in &catalog.packages {
        for (path, bytes) in files.iter() {
            let relative = Path::new(&package_dir(id)).join(path);
            let mut current = dir.to_path_buf();
            if let Some(parent) = relative.parent() {
                for component in parent.components() {
                    if let Component::Normal(name) = component {
                        current.push(name);
                        create_if_missing(fs, &current)?;
                    }
                }
            }
            fs.write_file_atomic(&dir.join(&relative), bytes)?;
        }
    }
    Ok(())
}

/// Creates one folder level with mode 0700 unless it exists. Its parent
/// must exist: the builder never walks above the folder it was given, so
/// it stays inside a test root.
fn create_if_missing(fs: &dyn Filesystem, path: &Path) -> Result<(), PlatformError> {
    if fs.symlink_metadata(path)?.is_some() {
        return Ok(());
    }
    match fs.create_dir_private(path) {
        Ok(()) | Err(PlatformError::AlreadyExists { .. }) => Ok(()),
        Err(err) => Err(err),
    }
}
