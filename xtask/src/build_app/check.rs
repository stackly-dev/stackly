//! The check of what a staged bundle holds (spec 0015, step 4), before
//! anything is signed.

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};
use stackly_platform::bundle::{
    APP_EXECUTABLE, CLI_EXECUTABLE, DAEMON_EXECUTABLE, DAEMON_PLIST_FILE, LAUNCH_AGENTS_DIR,
    MACOS_DIR, RESOURCES_DIR,
};

use super::capture;

/// The only executables allowed in `Contents/MacOS`.
const EXPECTED: [&str; 3] = [APP_EXECUTABLE, DAEMON_EXECUTABLE, CLI_EXECUTABLE];
/// The name of the test server, which never ships.
const FAKE_SERVER: &str = "stackly-fake-server";

/// Fails when two of the three executable names are the same file on a
/// case insensitive volume, which is how APFS formats a Mac by default:
/// copying `stackly` over `Stackly` would silently replace the app.
///
/// # Errors
///
/// The pair of names that collide.
pub fn names_are_distinct() -> Result<()> {
    distinct_ignoring_case(&EXPECTED)
}

fn distinct_ignoring_case(names: &[&str]) -> Result<()> {
    for (index, first) in names.iter().enumerate() {
        for second in &names[index + 1..] {
            if first.eq_ignore_ascii_case(second) {
                bail!(
                    "Contents/MacOS cannot hold both `{first}` and `{second}`: they are one file on a case insensitive volume (APFS default), so the second would replace the first; the bundle layout of spec 0015 needs a decision"
                );
            }
        }
    }
    Ok(())
}

/// Checks the staged `bundle` for the workspace `version`.
///
/// # Errors
///
/// What is wrong with the bundle, named.
pub fn contents(bundle: &Path, version: &str) -> Result<()> {
    names_are_distinct()?;
    only_expected_executables(bundle, &EXPECTED)?;
    for name in EXPECTED {
        let path = bundle.join(MACOS_DIR).join(name);
        executable_is_arm64_and_0755(&path)?;
    }
    no_test_files(bundle)?;
    if !bundle
        .join(LAUNCH_AGENTS_DIR)
        .join(DAEMON_PLIST_FILE)
        .is_file()
    {
        bail!("the bundle has no LaunchAgent property list");
    }
    if !bundle
        .join(RESOURCES_DIR)
        .join("man/man1/stackly.1")
        .is_file()
    {
        bail!("the bundle has no man page");
    }
    versions_agree(bundle, version)
}

fn only_expected_executables(bundle: &Path, expected: &[&str]) -> Result<()> {
    let mut found: Vec<String> = std::fs::read_dir(bundle.join(MACOS_DIR))
        .context("the bundle has no Contents/MacOS")?
        .map(|entry| entry.map(|entry| entry.file_name().to_string_lossy().into_owned()))
        .collect::<std::io::Result<_>>()?;
    found.sort();
    let mut expected: Vec<String> = expected.iter().map(|name| (*name).to_owned()).collect();
    expected.sort();
    if found != expected {
        bail!("Contents/MacOS holds {found:?}, but only {expected:?} may be there");
    }
    Ok(())
}

fn executable_is_arm64_and_0755(path: &Path) -> Result<()> {
    let mode = std::fs::metadata(path)?.permissions().mode() & 0o777;
    if mode != 0o755 {
        bail!("{} has mode {mode:o}, not 755", path.display());
    }
    let archs = capture(Command::new("lipo").arg("-archs").arg(path))?;
    if archs != "arm64" {
        bail!("{} holds `{archs}`, not arm64", path.display());
    }
    Ok(())
}

/// No `stackly-fake-server` anywhere, and no path segment named `testing`
/// under the bundled catalog.
fn no_test_files(bundle: &Path) -> Result<()> {
    let mut stack = vec![bundle.to_path_buf()];
    let catalog = bundle.join(RESOURCES_DIR).join("catalog");
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if name == FAKE_SERVER {
                bail!("{} must not ship", path.display());
            }
            if path.starts_with(&catalog) && name == "testing" {
                bail!("{} is a testing package in the catalog", path.display());
            }
            if entry.file_type()?.is_dir() {
                stack.push(path);
            }
        }
    }
    Ok(())
}

/// `CFBundleShortVersionString`, `stacklyd --version`, and `stackly
/// --version` all name the workspace version.
fn versions_agree(bundle: &Path, version: &str) -> Result<()> {
    let info = bundle.join("Contents/Info.plist");
    let short = capture(
        Command::new("plutil")
            .args(["-extract", "CFBundleShortVersionString", "raw", "-o", "-"])
            .arg(&info),
    )?;
    if short != version {
        bail!("CFBundleShortVersionString is {short}, but the workspace version is {version}");
    }
    for name in [DAEMON_EXECUTABLE, CLI_EXECUTABLE] {
        let printed = capture(Command::new(bundle.join(MACOS_DIR).join(name)).arg("--version"))?;
        if !printed.split_whitespace().any(|word| word == version) {
            bail!("{name} --version says `{printed}`, which lacks the workspace version {version}");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_bundle(dir: &Path) -> std::path::PathBuf {
        let bundle = dir.join("Stackly.app");
        std::fs::create_dir_all(bundle.join(MACOS_DIR)).unwrap();
        for name in [APP_EXECUTABLE, DAEMON_EXECUTABLE] {
            std::fs::write(bundle.join(MACOS_DIR).join(name), b"").unwrap();
        }
        bundle
    }

    #[test]
    fn names_that_differ_only_in_case_are_refused() {
        distinct_ignoring_case(&["Stackly", "stacklyd", "stackly-cli"]).unwrap();
        let error = distinct_ignoring_case(&["Stackly", "stacklyd", "stackly"])
            .unwrap_err()
            .to_string();
        assert!(error.contains("`Stackly` and `stackly`"), "{error}");
    }

    #[test]
    fn only_the_three_executables_may_sit_in_contents_macos() {
        let dir = tempfile::tempdir().unwrap();
        let bundle = dir.path().join("Stackly.app");
        std::fs::create_dir_all(bundle.join(MACOS_DIR)).unwrap();
        // File names that differ by more than case, whatever the real ones are.
        let expected = ["app", "daemon", "cli"];
        for name in expected {
            std::fs::write(bundle.join(MACOS_DIR).join(name), b"").unwrap();
        }
        only_expected_executables(&bundle, &expected).unwrap();

        std::fs::write(bundle.join(MACOS_DIR).join("Stackly.debug.dylib"), b"").unwrap();
        let error = only_expected_executables(&bundle, &expected)
            .unwrap_err()
            .to_string();
        assert!(error.contains("Stackly.debug.dylib"), "{error}");

        std::fs::remove_file(bundle.join(MACOS_DIR).join("Stackly.debug.dylib")).unwrap();
        std::fs::remove_file(bundle.join(MACOS_DIR).join("cli")).unwrap();
        assert!(only_expected_executables(&bundle, &expected).is_err());
    }

    #[test]
    fn the_fake_server_and_a_testing_package_in_the_catalog_never_ship() {
        let dir = tempfile::tempdir().unwrap();
        let bundle = fake_bundle(dir.path());
        no_test_files(&bundle).unwrap();

        // `testing` outside the catalog is not a catalog package.
        std::fs::create_dir_all(bundle.join("Contents/Resources/man/testing")).unwrap();
        no_test_files(&bundle).unwrap();

        std::fs::create_dir_all(bundle.join("Contents/Resources/catalog/testing/fake-command"))
            .unwrap();
        assert!(no_test_files(&bundle).is_err());
        std::fs::remove_dir_all(bundle.join("Contents/Resources/catalog/testing")).unwrap();

        std::fs::write(bundle.join(MACOS_DIR).join(FAKE_SERVER), b"").unwrap();
        assert!(no_test_files(&bundle).is_err());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_mode_other_than_0755_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let bundle = fake_bundle(dir.path());
        let path = bundle.join(MACOS_DIR).join(DAEMON_EXECUTABLE);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        let error = executable_is_arm64_and_0755(&path).unwrap_err().to_string();
        assert!(error.contains("755"), "{error}");
    }
}
