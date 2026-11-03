//! A locked Keychain at login (spec 0031 AC-15) against a real file
//! keychain in the test root: the login start meets the locked store, the
//! instance stays `stopped` with desired `running` and a `needsSecret`
//! blocker, and the supervisor starts it with the trigger `secret_retry`
//! when the keychain opens. The login Keychain is never touched.
//!
//! The test is native: it needs a GUI login session to lock a keychain, so it
//! runs only in a native run you start: `cargo xtask evidence run --kind
//! native -- cargo test -p stacklyd --test supervisor native_keychain --
//! --ignored --test-threads 1`. Without one it ends `BLOCKED`.

use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use serde_json::json;
use stackly_platform_macos::{KeychainTarget, MacosSecretStore};
use stackly_protocol::instance::{InstanceBlocker, InstanceLifecycle, InstanceReadinessState};
use stackly_testkit::FakeLoginSessionInfo;
use stackly_testkit::evidence::{Kind, Source, evidence};

use crate::login::{launchd, set_at_login, start, stop};
use crate::support::Site;

/// The process IDs of `SecurityAgent`, the process that shows the Keychain
/// dialog. Only read; nothing is signalled.
fn security_agents() -> Vec<String> {
    let output = Command::new("/usr/bin/pgrep")
        .args(["-x", "SecurityAgent"])
        .output()
        .expect("pgrep runs");
    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .map(str::to_owned)
        .collect()
}

/// `/usr/bin/security <verb> ...` on the file keychain.
fn security(args: &[&str]) {
    let output = Command::new("/usr/bin/security")
        .args(args)
        .output()
        .expect("security runs");
    assert!(
        output.status.success(),
        "security {} failed: {}",
        args[0],
        String::from_utf8_lossy(&output.stderr)
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: a file keychain locked with security lock-keychain"]
async fn a_locked_file_keychain_holds_the_login_start_and_the_retry_starts_it_when_it_opens() {
    let ev = evidence!(
        covers = ["AC-062", "REQ-016"],
        kind = Kind::Native,
        expected = "with the file keychain of a temporary root locked by security lock-keychain, a login start leaves the at_login instance stopped with desired running and a needsSecret blocker, no dialog shows, and within a few retry intervals after security unlock-keychain the instance runs with the blocker gone",
    );
    let manager = Command::new("launchctl")
        .arg("managername")
        .output()
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .unwrap_or_default();
    if manager != "Aqua" {
        ev.blocked(
            &format!(
                "no GUI login session (launchd manager `{manager}`), which locking a keychain needs"
            ),
            "the user of this Mac, logged in to a GUI session",
        );
        return;
    }
    let macos = Command::new("/usr/bin/sw_vers")
        .arg("-productVersion")
        .output()
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .expect("sw_vers runs");
    ev.subject("macos-keychain", &macos, None, Source::External);
    let mut site = Site::new();
    let dir = site.root.path().join("keychain-native");
    let target = KeychainTarget::in_dir(&dir);
    let store = MacosSecretStore::new(target).expect("the file keychain is made in the test root");
    site.platform = site.platform.clone().with_secrets(Arc::new(store));
    let keychain = dir.join("stackly.keychain-db");
    let password = std::fs::read_to_string(dir.join("stackly.keychain-password"))
        .expect("the password file of the test keychain");
    let session = FakeLoginSessionInfo::new();

    let manual = site.start().await;
    let id = {
        let api = site.api().await;
        api.create_ready("pg", json!({})).await
    };
    stop(manual).await;
    set_at_login(&site, id);
    let first = start(&site, launchd(), &session).await;
    stop(first).await;

    let agents_before = security_agents();
    security(&["lock-keychain", path_text(&keychain)]);
    session.set_audit_session(100_002);
    let second = start(&site, launchd(), &session).await;
    let api = site.api().await;
    let blocked = api
        .wait_instance(id, "a needsSecret blocker", |summary| {
            matches!(summary.blocker, Some(InstanceBlocker::NeedsSecret(_)))
        })
        .await;
    assert_eq!(blocked.lifecycle, InstanceLifecycle::Stopped);
    // launchd starts SecurityAgent a few milliseconds after securityd asks
    // for it, so the list is read after a pause.
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    assert_eq!(
        security_agents(),
        agents_before,
        "a locked keychain read with no interaction allowed shows no dialog"
    );

    security(&[
        "unlock-keychain",
        "-p",
        password.trim(),
        path_text(&keychain),
    ]);
    let running = api
        .wait_instance(
            id,
            "the instance to run after the keychain opened",
            |summary| summary.readiness.state == InstanceReadinessState::Ready,
        )
        .await;
    assert!(running.blocker.is_none(), "{:?}", running.blocker);
    api.stop_when_free(id).await;
    drop(api);
    stop(second).await;
    ev.pass_with(
        "locked file keychain: the login start left the instance stopped with a needsSecret blocker; after unlock-keychain the secret_retry start ran it and the blocker went",
    );
}

fn path_text(path: &Path) -> &str {
    path.to_str().expect("the test root path is UTF-8")
}
