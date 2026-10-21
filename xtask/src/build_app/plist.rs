//! The LaunchAgent property list of the daemon (spec 0015, *Plist
//! LaunchAgent*), rendered from the identifiers in `stackly-platform` and
//! never written by hand. `SMAppService` registers it from the bundle at
//! `Contents/Library/LaunchAgents`, and `BundleProgram` is relative to the
//! bundle.

use anyhow::{Context, Result, bail};
use stackly_platform::bundle::{
    APP_BUNDLE_ID, DAEMON_BUNDLE_PROGRAM, DAEMON_LABEL, LAUNCHD_THROTTLE_INTERVAL_SECS,
};
use std::path::Path;
use std::process::Command;

/// How long launchd waits for the daemon to stop before it kills it; longer
/// than the stop order of spec 0006, which takes about 12 seconds at most.
const EXIT_TIMEOUT_SECS: u64 = 30;

/// Renders the property list.
#[must_use]
pub fn render() -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
<plist version=\"1.0\">\n\
<dict>\n\
\t<key>Label</key>\n\
\t<string>{DAEMON_LABEL}</string>\n\
\t<key>BundleProgram</key>\n\
\t<string>{DAEMON_BUNDLE_PROGRAM}</string>\n\
\t<key>AssociatedBundleIdentifiers</key>\n\
\t<array>\n\
\t\t<string>{APP_BUNDLE_ID}</string>\n\
\t</array>\n\
\t<key>RunAtLoad</key>\n\
\t<true/>\n\
\t<key>KeepAlive</key>\n\
\t<dict>\n\
\t\t<key>SuccessfulExit</key>\n\
\t\t<false/>\n\
\t</dict>\n\
\t<key>ThrottleInterval</key>\n\
\t<integer>{LAUNCHD_THROTTLE_INTERVAL_SECS}</integer>\n\
\t<key>ExitTimeOut</key>\n\
\t<integer>{EXIT_TIMEOUT_SECS}</integer>\n\
\t<key>ProcessType</key>\n\
\t<string>Standard</string>\n\
</dict>\n\
</plist>\n"
    )
}

/// Runs `plutil -lint` on `path`.
pub fn lint(path: &Path) -> Result<()> {
    let output = Command::new("plutil")
        .arg("-lint")
        .arg(path)
        .output()
        .context("cannot run plutil")?;
    if !output.status.success() {
        bail!(
            "plutil -lint refused {}: {}{}",
            path.display(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn golden_path() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/golden/com.ojiepermana.stackly.daemon.plist")
    }

    #[test]
    fn the_rendered_plist_is_the_golden_file() {
        let rendered = render();
        if std::env::var_os("STACKLY_UPDATE_GOLDEN").is_some_and(|value| value == "1") {
            std::fs::write(golden_path(), &rendered).expect("the golden file can be written");
            return;
        }
        let golden = std::fs::read_to_string(golden_path())
            .expect("the golden plist exists; run with STACKLY_UPDATE_GOLDEN=1");
        assert_eq!(rendered, golden);
    }

    #[test]
    fn the_throttle_interval_comes_from_the_constant_the_cli_counts_with() {
        assert!(render().contains(&format!(
            "<key>ThrottleInterval</key>\n\t<integer>{LAUNCHD_THROTTLE_INTERVAL_SECS}</integer>"
        )));
    }

    #[test]
    fn the_keys_are_exactly_the_spec_table_and_nothing_that_it_forbids() {
        let text = render();
        let keys: Vec<&str> = text
            .lines()
            .filter_map(|line| line.trim().strip_prefix("<key>"))
            .filter_map(|line| line.strip_suffix("</key>"))
            .collect();
        assert_eq!(
            keys,
            [
                "Label",
                "BundleProgram",
                "AssociatedBundleIdentifiers",
                "RunAtLoad",
                "KeepAlive",
                "SuccessfulExit",
                "ThrottleInterval",
                "ExitTimeOut",
                "ProcessType"
            ]
        );
        for forbidden in [
            "ProgramArguments",
            "EnvironmentVariables",
            "StandardOutPath",
            "StandardErrorPath",
            "MachServices",
            "Sockets",
            "AbandonProcessGroup",
            "LimitLoadToSessionType",
        ] {
            assert!(!text.contains(forbidden), "{forbidden}");
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_rendered_plist_passes_plutil_lint() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let path = dir.path().join("daemon.plist");
        std::fs::write(&path, render()).expect("the plist can be written");
        lint(&path).expect("plutil accepts the plist");
    }
}
