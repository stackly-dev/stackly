//! `cargo xtask provider freeze <dir>` and the `not frozen` check (spec
//! 0036, *Corpus kompatibilitas*, AC-4, AC-6).
//!
//! An entry is `crates/core/tests/fixtures/compat/<name>/<revision>/` with
//! `package/`, a byte copy of every file `read_package` reads plus
//! `tests/scenarios.yaml`, and `frozen.json` next to it, written from
//! `stackly_core::provider::testing::frozen_facts`. An entry is never
//! changed or removed: the same revision with another digest is refused.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use stackly_core::provider::testing::{FrozenFacts, frozen_facts};
use stackly_core::provider::{NameCheck, ValidatedPackage, check_package, read_scenarios};
use stackly_platform_macos::MacosFilesystem;

/// The corpus, relative to the repository root.
pub const CORPUS_DIR: &str = "crates/core/tests/fixtures/compat";
/// The file next to `package/` in an entry.
const FROZEN_FILE: &str = "frozen.json";

/// What a freeze did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Frozen {
    /// A new entry was written at this folder.
    Written(PathBuf),
    /// The entry already holds this revision with the same digest.
    Already(PathBuf),
}

/// Freezes the package in `dir` into `corpus`. `catalog` holds the
/// `[[provider]]` IDs of `providers/catalog.toml` ([`catalog_providers`]):
/// only a provider in the catalog is pinned in a user's root, so only it
/// is frozen.
///
/// # Errors
///
/// Fails, writing nothing, for a package that is not publishable, one
/// outside the `official` namespace, one that is not a `[[provider]]` of
/// the catalog (such as a `[[planned]]` one), or a revision already frozen
/// with another digest; and when a file cannot be written.
pub fn freeze(corpus: &Path, catalog: &[String], dir: &Path) -> Result<Frozen> {
    let check = check_package(&MacosFilesystem, dir, NameCheck::FolderName);
    if !check.publishable() {
        let lines: Vec<String> = check
            .findings
            .iter()
            .map(|finding| format!("{}: {}", check.folder, finding.issue))
            .collect();
        bail!(
            "{} is not publishable, so it cannot be frozen:\n{}",
            dir.display(),
            lines.join("\n")
        );
    }
    let package = check
        .package
        .as_ref()
        .context("a publishable package has validated")?;
    let key = package.key();
    if key.provider().namespace() != "official" {
        bail!(
            "{} is {}: only official/ packages are frozen, because only they are pinned in a user's root",
            dir.display(),
            key.provider()
        );
    }
    if !catalog
        .iter()
        .any(|id| id.as_str() == key.provider().as_str())
    {
        bail!(
            "{} is not a [[provider]] of providers/catalog.toml: only a provider in the catalog is pinned in a user's root, so freeze it in the commit that moves it from [[planned]] to [[provider]]",
            key.provider()
        );
    }
    let facts = frozen_facts(package, &check.scenarios)
        .with_context(|| format!("cannot build the frozen plans of {}", key.provider()))?;
    let entry = entry_dir(corpus, package);
    if entry.exists() {
        let recorded = read_facts(&entry)?;
        if recorded.manifest_sha256 == facts.manifest_sha256 {
            return Ok(Frozen::Already(entry));
        }
        bail!(
            "{} revision {} is frozen with manifest_sha256 {}, but the package now has {}: raise metadata.revision, since a frozen entry never changes",
            key.provider(),
            key.revision(),
            recorded.manifest_sha256,
            facts.manifest_sha256
        );
    }
    let parent = entry.parent().context("an entry has a parent folder")?;
    fs::create_dir_all(parent).with_context(|| format!("cannot create {}", parent.display()))?;
    let staging = parent.join(format!(".{}.partial", key.revision()));
    if staging.exists() {
        fs::remove_dir_all(&staging)
            .with_context(|| format!("cannot clear {}", staging.display()))?;
    }
    write_entry(&staging, dir, package, &facts)?;
    fs::rename(&staging, &entry)
        .with_context(|| format!("cannot move the entry to {}", entry.display()))?;
    Ok(Frozen::Written(entry))
}

fn write_entry(
    staging: &Path,
    dir: &Path,
    package: &ValidatedPackage,
    facts: &FrozenFacts,
) -> Result<()> {
    let target = staging.join("package");
    for (path, bytes) in package.files().iter() {
        write(&target.join(path), bytes)?;
    }
    let scenarios =
        read_scenarios(&MacosFilesystem, dir).map_err(|issue| anyhow::anyhow!("{issue}"))?;
    if let Some(text) = scenarios {
        write(&target.join("tests/scenarios.yaml"), text.as_bytes())?;
    }
    let mut json = serde_json::to_string_pretty(facts).context("frozen facts encode")?;
    json.push('\n');
    write(&staging.join(FROZEN_FILE), json.as_bytes())
}

fn write(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("cannot create {}", parent.display()))?;
    }
    fs::write(path, bytes).with_context(|| format!("cannot write {}", path.display()))
}

/// `<corpus>/<name>/<revision>`.
fn entry_dir(corpus: &Path, package: &ValidatedPackage) -> PathBuf {
    let key = package.key();
    corpus
        .join(key.provider().name())
        .join(key.revision().to_string())
}

fn read_facts(entry: &Path) -> Result<FrozenFacts> {
    let path = entry.join(FROZEN_FILE);
    let text =
        fs::read_to_string(&path).with_context(|| format!("cannot read {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("{} is not frozen facts", path.display()))
}

/// Whether `corpus` holds `package`'s revision with its digest.
#[must_use]
pub fn is_frozen(corpus: &Path, package: &ValidatedPackage) -> bool {
    read_facts(&entry_dir(corpus, package))
        .is_ok_and(|facts| facts.manifest_sha256 == package.key().manifest_sha256().to_string())
}

/// The `[[provider]]` IDs of `providers/catalog.toml`, in file order.
///
/// # Errors
///
/// Fails when the file cannot be read or does not parse.
pub fn catalog_providers(root: &Path) -> Result<Vec<String>> {
    let path = root.join("providers/catalog.toml");
    let text =
        fs::read_to_string(&path).with_context(|| format!("cannot read {}", path.display()))?;
    let value: toml::Table =
        toml::from_str(&text).with_context(|| format!("{} is invalid", path.display()))?;
    let providers = value
        .get("provider")
        .and_then(toml::Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    Ok(providers
        .iter()
        .filter_map(|provider| provider.get("id")?.as_str().map(str::to_owned))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
    }

    fn copy_dir(from: &Path, to: &Path) {
        fs::create_dir_all(to).unwrap();
        for entry in fs::read_dir(from).unwrap() {
            let entry = entry.unwrap();
            let target = to.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                copy_dir(&entry.path(), &target);
            } else {
                fs::copy(entry.path(), &target).unwrap();
            }
        }
    }

    // covers: AC-6 (spec 0036)
    #[test]
    fn a_new_entry_holds_the_package_bytes_and_its_facts_and_never_changes() {
        let root = stackly_testkit::TestRoot::new().unwrap();
        let corpus = root.path().join("compat");
        let nats = root.path().join("providers/nats");
        copy_dir(&repo().join("providers/nats"), &nats);

        let first = freeze(&corpus, &catalog(), &nats).unwrap();

        let Frozen::Written(entry) = first else {
            panic!("a new entry is written: {first:?}");
        };
        for file in [
            "provider.yaml",
            "versions.json",
            "schemas/settings.schema.json",
        ] {
            assert_eq!(
                fs::read(entry.join("package").join(file)).unwrap(),
                fs::read(nats.join(file)).unwrap(),
                "{file}"
            );
        }
        let facts = read_facts(&entry).unwrap();
        assert_eq!(facts.provider, "official/nats");
        assert_eq!(facts.schema, "stackly.provider-frozen/v1");
        assert!(!facts.start_args.is_empty());
        assert_eq!(
            freeze(&corpus, &catalog(), &nats).unwrap(),
            Frozen::Already(entry.clone())
        );

        let manifest = nats.join("provider.yaml");
        let text = fs::read_to_string(&manifest).unwrap();
        fs::write(
            &manifest,
            text.replacen("displayName: NATS Server", "displayName: NATS", 1),
        )
        .unwrap();
        let before = fs::read(entry.join(FROZEN_FILE)).unwrap();
        let err = freeze(&corpus, &catalog(), &nats).unwrap_err().to_string();
        assert!(err.contains("raise metadata.revision"), "{err}");
        assert_eq!(fs::read(entry.join(FROZEN_FILE)).unwrap(), before);
        assert!(fs::read_dir(entry.parent().unwrap()).unwrap().all(|e| {
            !e.unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".partial")
        }));
    }

    // covers: AC-6 (spec 0036)
    #[test]
    fn testing_and_unpublishable_packages_are_refused_without_a_write() {
        let root = stackly_testkit::TestRoot::new().unwrap();
        let corpus = root.path().join("compat");

        let fake = freeze(&corpus, &catalog(), &repo().join("providers/fake")).unwrap_err();
        let command = freeze(
            &corpus,
            &catalog(),
            &repo().join("crates/core/tests/fixtures/packages/fake-command"),
        )
        .unwrap_err();

        assert!(
            fake.to_string().contains("only official/ packages"),
            "{fake}"
        );
        assert!(command.to_string().contains("not publishable"), "{command}");
        assert!(!corpus.exists());
    }

    fn catalog() -> Vec<String> {
        catalog_providers(&repo()).unwrap()
    }

    // covers: AC-6 (spec 0036)
    #[test]
    fn a_package_that_is_not_a_catalog_provider_is_refused_without_a_write() {
        let root = stackly_testkit::TestRoot::new().unwrap();
        let corpus = root.path().join("compat");
        let traefik = root.path().join("providers/traefik");
        copy_dir(&repo().join("providers/traefik"), &traefik);

        let planned = freeze(&corpus, &catalog(), &traefik)
            .unwrap_err()
            .to_string();
        let listed = freeze(
            &corpus,
            &["official/nats".to_owned()],
            &repo().join("providers/postgresql"),
        )
        .unwrap_err()
        .to_string();

        assert!(
            planned.contains("official/traefik is not a [[provider]] of providers/catalog.toml"),
            "{planned}"
        );
        assert!(
            listed.contains("official/postgresql is not a [[provider]]"),
            "{listed}"
        );
        assert!(!corpus.exists());
    }

    #[test]
    fn the_catalog_lists_its_provider_ids_in_order() {
        let ids = catalog_providers(&repo()).unwrap();
        assert!(ids.contains(&"official/nats".to_owned()), "{ids:?}");
        assert!(ids.iter().all(|id| id.starts_with("official/")));
    }
}
