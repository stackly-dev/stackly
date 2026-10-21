//! `build-app --install [<dir>]` (spec 0015, AC-22): puts the built bundle
//! into a development folder, `~/Applications` by default, without ever
//! leaving `<dir>/Stackly.app` half made and without touching a bundle that
//! is not Stackly.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, anyhow, bail};
use stackly_platform::bundle::APP_BUNDLE_ID;

use super::{BUNDLE_NAME, capture, ditto};

/// `~/Applications`.
///
/// # Errors
///
/// `HOME` is not set.
pub fn default_install_dir() -> Result<PathBuf> {
    let home = std::env::var_os("HOME").ok_or_else(|| anyhow!("HOME is not set"))?;
    Ok(PathBuf::from(home).join("Applications"))
}

/// Installs `bundle` as `dir/Stackly.app`.
///
/// # Errors
///
/// An existing `dir/Stackly.app` that is not the Stackly app (untouched), or
/// a copy or rename that fails (staging removed, old bundle put back).
pub fn install(bundle: &Path, dir: &Path) -> Result<()> {
    let target = dir.join(BUNDLE_NAME);
    refuse_a_foreign_bundle(&target)?;
    std::fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
    replace(bundle, dir)?;
    println!("installed {}", target.display());
    if let Some(hint) = restart_hint(default_socket_exists()) {
        println!("{hint}");
    }
    Ok(())
}

/// Refuses a folder at `target` whose `CFBundleIdentifier` is not the
/// Stackly app's, and one that has none.
fn refuse_a_foreign_bundle(target: &Path) -> Result<()> {
    if std::fs::symlink_metadata(target).is_err() {
        return Ok(());
    }
    let info = target.join("Contents/Info.plist");
    let identifier = capture(
        Command::new("plutil")
            .args(["-extract", "CFBundleIdentifier", "raw", "-o", "-"])
            .arg(&info),
    )
    .unwrap_or_default();
    if identifier != APP_BUNDLE_ID {
        bail!(
            "{} is not the Stackly app (its bundle identifier is `{identifier}`), so it was not touched",
            target.display()
        );
    }
    Ok(())
}

/// Copies `bundle` to a staging name next to the target, moves the old
/// bundle aside, renames the staging bundle in, and removes the old one.
fn replace(bundle: &Path, dir: &Path) -> Result<()> {
    let pid = std::process::id();
    let target = dir.join(BUNDLE_NAME);
    let staging = dir.join(format!(".{BUNDLE_NAME}.staging-{pid}"));
    let old = dir.join(format!(".{BUNDLE_NAME}.old-{pid}"));
    let _ = std::fs::remove_dir_all(&staging);
    if let Err(error) = ditto(bundle, &staging) {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(error);
    }
    let had_old = std::fs::symlink_metadata(&target).is_ok();
    if had_old && let Err(error) = std::fs::rename(&target, &old) {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(error).with_context(|| format!("cannot move {} aside", target.display()));
    }
    if let Err(error) = std::fs::rename(&staging, &target) {
        let _ = std::fs::remove_dir_all(&staging);
        if had_old {
            let _ = std::fs::rename(&old, &target);
        }
        return Err(error)
            .with_context(|| format!("cannot move the bundle to {}", target.display()));
    }
    if had_old {
        std::fs::remove_dir_all(&old)
            .with_context(|| format!("cannot remove {}", old.display()))?;
    }
    Ok(())
}

/// The hint to print after an install, when a daemon of the default root
/// may still run from the old bundle.
#[must_use]
fn restart_hint(socket_exists: bool) -> Option<&'static str> {
    socket_exists.then_some("run stackly daemon restart to use the new stacklyd")
}

/// Whether the daemon socket of the default root exists. `STACKLY_ROOT` is
/// not looked at: the agent only serves the default root.
#[cfg(target_os = "macos")]
fn default_socket_exists() -> bool {
    use stackly_platform_macos::MacosPlatform;

    MacosPlatform::new(None)
        .ok()
        .and_then(|platform| platform.directories().roots().ok())
        .is_some_and(|roots| {
            stackly_platform::DirectoryLayout::new(roots)
                .daemon_socket()
                .exists()
        })
}

#[cfg(not(target_os = "macos"))]
fn default_socket_exists() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A bundle folder with an `Info.plist` naming `identifier` and a file
    /// that says which one it is.
    fn bundle(path: &Path, identifier: &str, marker: &str) {
        std::fs::create_dir_all(path.join("Contents")).unwrap();
        std::fs::write(
            path.join("Contents/Info.plist"),
            format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<plist version=\"1.0\"><dict><key>CFBundleIdentifier</key><string>{identifier}</string></dict></plist>\n"
            ),
        )
        .unwrap();
        std::fs::write(path.join("Contents/marker"), marker).unwrap();
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_folder_that_is_not_stackly_is_refused_and_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let built = dir.path().join("built/Stackly.app");
        bundle(&built, APP_BUNDLE_ID, "new");
        let apps = dir.path().join("Applications");
        bundle(&apps.join(BUNDLE_NAME), "com.example.other", "foreign");

        let error = install(&built, &apps).unwrap_err().to_string();

        assert!(error.contains("com.example.other"), "{error}");
        assert_eq!(
            std::fs::read_to_string(apps.join(BUNDLE_NAME).join("Contents/marker")).unwrap(),
            "foreign"
        );
        assert_eq!(names(&apps), [BUNDLE_NAME], "nothing was added");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_folder_without_an_info_plist_is_refused_too() {
        let dir = tempfile::tempdir().unwrap();
        let built = dir.path().join("built/Stackly.app");
        bundle(&built, APP_BUNDLE_ID, "new");
        let apps = dir.path().join("Applications");
        std::fs::create_dir_all(apps.join(BUNDLE_NAME).join("Contents")).unwrap();

        assert!(install(&built, &apps).is_err());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn an_old_stackly_bundle_is_replaced_and_no_hidden_folder_is_left() {
        let dir = tempfile::tempdir().unwrap();
        let built = dir.path().join("built/Stackly.app");
        bundle(&built, APP_BUNDLE_ID, "new");
        let apps = dir.path().join("Applications");
        bundle(&apps.join(BUNDLE_NAME), APP_BUNDLE_ID, "old");

        install(&built, &apps).unwrap();

        assert_eq!(
            std::fs::read_to_string(apps.join(BUNDLE_NAME).join("Contents/marker")).unwrap(),
            "new"
        );
        assert_eq!(
            names(&apps),
            [BUNDLE_NAME],
            "no .Stackly.app.* folder remains"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_folder_is_made_when_it_does_not_exist() {
        let dir = tempfile::tempdir().unwrap();
        let built = dir.path().join("built/Stackly.app");
        bundle(&built, APP_BUNDLE_ID, "new");
        let apps = dir.path().join("a/b/Applications");

        install(&built, &apps).unwrap();

        assert!(apps.join(BUNDLE_NAME).join("Contents/marker").is_file());
    }

    #[test]
    fn the_restart_hint_shows_only_when_the_default_socket_exists() {
        assert_eq!(
            restart_hint(true),
            Some("run stackly daemon restart to use the new stacklyd")
        );
        assert_eq!(restart_hint(false), None);
    }
}
