//! Assembly of the bundle in the staging folder (spec 0015, *Build,
//! assembly, dan signing*, step 3): an ordered list of steps, each of which
//! may fail the whole build.

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};
use stackly_platform::bundle::{
    CLI_EXECUTABLE, DAEMON_EXECUTABLE, DAEMON_PLIST_FILE, LAUNCH_AGENTS_DIR, MACOS_DIR,
};

use super::{Binaries, plist};

/// The folder of the man pages, relative to the bundle.
const MAN_DIR: &str = "Contents/Resources/man/man1";

/// The folder of the bundled catalog, relative to the bundle.
const CATALOG_DIR: &str = "Contents/Resources/catalog";

/// What the steps build from.
struct Inputs<'a> {
    /// The executables `cargo build` made.
    binaries: &'a Binaries,
    /// The repository root, where `providers/catalog.toml` lives.
    root: &'a Path,
}

/// One assembly step.
type Step = fn(&Path, &Inputs) -> Result<()>;

/// The steps, in order. The catalog step of spec 0017 (milestone 4, step 11)
/// goes after the plist and before the man pages: it builds the catalog into
/// `Contents/Resources/catalog` in process, and a failure stops the build
/// like any other step.
const STEPS: &[(&str, Step)] = &[
    ("copy the executables", copy_binaries),
    ("write the LaunchAgent plist", write_plist),
    ("write the provider catalog", write_catalog),
    ("write the man pages", write_man_pages),
];

/// Runs every step on the staged `bundle`; `root` is the repository root.
///
/// # Errors
///
/// The first step that fails, named in the message.
pub fn assemble(bundle: &Path, binaries: &Binaries, root: &Path) -> Result<()> {
    run_steps(STEPS, bundle, &Inputs { binaries, root })
}

fn run_steps(steps: &[(&str, Step)], bundle: &Path, inputs: &Inputs) -> Result<()> {
    for (name, step) in steps {
        step(bundle, inputs).with_context(|| format!("assembly step failed: {name}"))?;
    }
    Ok(())
}

fn copy_binaries(bundle: &Path, inputs: &Inputs) -> Result<()> {
    let binaries = inputs.binaries;
    let macos = bundle.join(MACOS_DIR);
    std::fs::create_dir_all(&macos)?;
    for (from, name) in [
        (&binaries.daemon, DAEMON_EXECUTABLE),
        (&binaries.cli, CLI_EXECUTABLE),
    ] {
        let to = macos.join(name);
        std::fs::copy(from, &to)
            .with_context(|| format!("cannot copy {} to {}", from.display(), to.display()))?;
        std::fs::set_permissions(&to, std::fs::Permissions::from_mode(0o755))?;
    }
    Ok(())
}

fn write_plist(bundle: &Path, _inputs: &Inputs) -> Result<()> {
    let dir = bundle.join(LAUNCH_AGENTS_DIR);
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(DAEMON_PLIST_FILE);
    std::fs::write(&path, plist::render())?;
    plist::lint(&path)
}

/// Builds the catalog of `providers/catalog.toml` into
/// `Contents/Resources/catalog` in process (`xtask catalog build`): the same
/// check of the source, the packages, and the revision ledger, so a catalog
/// that `cargo xtask catalog check` refuses stops the build before anything
/// is signed. The folder holds `catalog.json` and the `official` packages
/// only; the daemon loads it as the bundled catalog.
fn write_catalog(bundle: &Path, inputs: &Inputs) -> Result<()> {
    crate::catalog::build(inputs.root, &bundle.join(CATALOG_DIR))?;
    Ok(())
}

/// Runs the new `stackly manpage --json` and writes each page.
fn write_man_pages(bundle: &Path, _inputs: &Inputs) -> Result<()> {
    let cli = bundle.join(MACOS_DIR).join(CLI_EXECUTABLE);
    let output = Command::new(&cli)
        .args(["manpage", "--json"])
        .env_remove("STACKLY_ROOT")
        .output()
        .with_context(|| format!("cannot run {}", cli.display()))?;
    if !output.status.success() {
        bail!(
            "stackly manpage failed with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    write_pages(&output.stdout, &bundle.join(MAN_DIR))
}

/// Writes the pages of a `stackly manpage --json` envelope into `dir`.
fn write_pages(envelope: &[u8], dir: &Path) -> Result<()> {
    let value: serde_json::Value =
        serde_json::from_slice(envelope).context("stackly manpage wrote no JSON")?;
    let pages = value["result"]["pages"]
        .as_array()
        .context("stackly manpage returned no pages")?;
    if pages.is_empty() {
        bail!("stackly manpage returned no pages");
    }
    std::fs::create_dir_all(dir)?;
    for page in pages {
        let file = page["file"].as_str().context("a page has no file name")?;
        let roff = page["roff"].as_str().context("a page has no text")?;
        if file.contains('/') || !file.ends_with(".1") {
            bail!("refused man page file name `{file}`");
        }
        std::fs::write(dir.join(file), roff)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs<'a>(binaries: &'a Binaries, root: &'a Path) -> Inputs<'a> {
        Inputs { binaries, root }
    }

    /// The repository root, two folders above `xtask`.
    fn repo_root() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
    }

    fn binaries(dir: &Path) -> Binaries {
        let daemon = dir.join("stacklyd-built");
        let cli = dir.join("stackly-built");
        std::fs::write(&daemon, b"daemon").unwrap();
        std::fs::write(&cli, b"cli").unwrap();
        Binaries { daemon, cli }
    }

    #[test]
    fn the_executables_are_copied_with_mode_0755_under_their_bundle_names() {
        let dir = tempfile::tempdir().unwrap();
        let bundle = dir.path().join("Stackly.app");
        copy_binaries(&bundle, &inputs(&binaries(dir.path()), dir.path())).unwrap();

        for name in [DAEMON_EXECUTABLE, CLI_EXECUTABLE] {
            let path = bundle.join(MACOS_DIR).join(name);
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o755, "{name}");
        }
        assert_eq!(
            std::fs::read(bundle.join(MACOS_DIR).join(DAEMON_EXECUTABLE)).unwrap(),
            b"daemon"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_plist_lands_where_smappservice_looks_for_it() {
        let dir = tempfile::tempdir().unwrap();
        let bundle = dir.path().join("Stackly.app");
        write_plist(&bundle, &inputs(&binaries(dir.path()), dir.path())).unwrap();
        let written =
            std::fs::read_to_string(bundle.join(LAUNCH_AGENTS_DIR).join(DAEMON_PLIST_FILE))
                .unwrap();
        assert_eq!(written, plist::render());
    }

    #[test]
    fn man_pages_are_written_from_the_envelope_and_a_bad_name_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let envelope = br#"{"result":{"pages":[{"file":"stackly.1","roff":".TH x"},{"file":"stackly-daemon.1","roff":".TH y"}]}}"#;
        write_pages(envelope, dir.path()).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.path().join("stackly.1")).unwrap(),
            ".TH x"
        );
        assert!(dir.path().join("stackly-daemon.1").is_file());

        let evil = br#"{"result":{"pages":[{"file":"../evil.1","roff":"x"}]}}"#;
        assert!(write_pages(evil, dir.path()).is_err());
        assert!(write_pages(br#"{"result":{"pages":[]}}"#, dir.path()).is_err());
        assert!(write_pages(b"not json", dir.path()).is_err());
    }

    #[test]
    fn a_failing_step_stops_the_ones_after_it_and_names_itself() {
        fn fail(_: &Path, _: &Inputs) -> Result<()> {
            bail!("no good")
        }
        fn never(_: &Path, _: &Inputs) -> Result<()> {
            panic!("a step after a failure must not run")
        }
        let dir = tempfile::tempdir().unwrap();
        let steps: &[(&str, Step)] = &[("the bad step", fail), ("the next step", never)];

        let built = binaries(dir.path());
        let error = run_steps(steps, dir.path(), &inputs(&built, dir.path())).unwrap_err();

        assert!(format!("{error:#}").contains("the bad step"), "{error:#}");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_catalog_step_writes_the_index_and_only_official_packages_into_the_bundle() {
        let dir = tempfile::tempdir().unwrap();
        let bundle = dir.path().join("Stackly.app");
        let built = binaries(dir.path());
        let root = repo_root();

        write_catalog(&bundle, &inputs(&built, &root)).unwrap();

        let catalog = bundle.join(CATALOG_DIR);
        assert!(catalog.join("catalog.json").is_file());
        let namespaces: Vec<_> = std::fs::read_dir(catalog.join("packages"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(namespaces, ["official"]);
        super::super::check::bundled_catalog(&bundle).unwrap();
    }

    #[test]
    fn a_failing_catalog_step_stops_the_build_and_leaves_no_bundle() {
        let dir = tempfile::tempdir().unwrap();
        let app_dir = dir.path().join("app");
        let output = app_dir.join("Debug");
        let built = binaries(dir.path());
        // A repository root without `providers/catalog.toml`: the catalog
        // step cannot read its source.
        let empty_root = dir.path().join("empty-root");
        std::fs::create_dir_all(&empty_root).unwrap();
        let steps: &[(&str, Step)] = &[
            ("copy the executables", copy_binaries),
            ("write the provider catalog", write_catalog),
            ("write the man pages", |_, _| {
                panic!("a step after the catalog must not run")
            }),
        ];

        let result = super::super::publish(&app_dir, &output, |stage| {
            let bundle = stage.join(super::super::BUNDLE_NAME);
            run_steps(steps, &bundle, &inputs(&built, &empty_root))
        });

        let error = format!("{:#}", result.unwrap_err());
        assert!(error.contains("write the provider catalog"), "{error}");
        assert!(error.contains("catalog.toml"), "{error}");
        assert!(!output.join(super::super::BUNDLE_NAME).exists());
        let leftovers: Vec<_> = std::fs::read_dir(&app_dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert!(
            leftovers.is_empty(),
            "no staging folder is left: {leftovers:?}"
        );
    }
}
