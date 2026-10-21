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

/// One assembly step.
type Step = fn(&Path, &Binaries) -> Result<()>;

/// The steps, in order. The catalog step of spec 0017 (milestone 4, step 11)
/// goes after the plist and before the man pages: it builds the catalog into
/// `Contents/Resources/catalog` in process, and a failure stops the build
/// like any other step. Until it exists the folder is absent and the daemon
/// reports no catalog.
const STEPS: &[(&str, Step)] = &[
    ("copy the executables", copy_binaries),
    ("write the LaunchAgent plist", write_plist),
    // catalog slot (spec 0017)
    ("write the man pages", write_man_pages),
];

/// Runs every step on the staged `bundle`.
///
/// # Errors
///
/// The first step that fails, named in the message.
pub fn assemble(bundle: &Path, binaries: &Binaries) -> Result<()> {
    run_steps(STEPS, bundle, binaries)
}

fn run_steps(steps: &[(&str, Step)], bundle: &Path, binaries: &Binaries) -> Result<()> {
    for (name, step) in steps {
        step(bundle, binaries).with_context(|| format!("assembly step failed: {name}"))?;
    }
    Ok(())
}

fn copy_binaries(bundle: &Path, binaries: &Binaries) -> Result<()> {
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

fn write_plist(bundle: &Path, _binaries: &Binaries) -> Result<()> {
    let dir = bundle.join(LAUNCH_AGENTS_DIR);
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(DAEMON_PLIST_FILE);
    std::fs::write(&path, plist::render())?;
    plist::lint(&path)
}

/// Runs the new `stackly manpage --json` and writes each page.
fn write_man_pages(bundle: &Path, _binaries: &Binaries) -> Result<()> {
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
        copy_binaries(&bundle, &binaries(dir.path())).unwrap();

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
        write_plist(&bundle, &binaries(dir.path())).unwrap();
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
        fn fail(_: &Path, _: &Binaries) -> Result<()> {
            bail!("no good")
        }
        fn never(_: &Path, _: &Binaries) -> Result<()> {
            panic!("a step after a failure must not run")
        }
        let dir = tempfile::tempdir().unwrap();
        let steps: &[(&str, Step)] = &[("the bad step", fail), ("the next step", never)];

        let error = run_steps(steps, dir.path(), &binaries(dir.path())).unwrap_err();

        assert!(format!("{error:#}").contains("the bad step"), "{error:#}");
    }
}
