//! The diagnostic bundle through the real `stacklyd` binary (spec 0034
//! AC-12, AC-13): `HOME` points into the test root, a preview of an earlier
//! run is left in `<cache>/diagnostics/`, and the daemon empties that folder
//! at start; an export logs one `diagnostics_exported` line whose path has
//! the home folder written `~`, while the reply keeps the absolute path.
#![cfg(target_os = "macos")]

mod common;

use std::io::Read;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use common::{STACKLYD, client, daemon_paths, handshakes, runtime};
use stackly_protocol::diagnostics::ExportBundleParams;
use stackly_protocol::{Call, Reply};
use stackly_testkit::TestRoot;
use stackly_testkit::evidence::{Kind, evidence};

/// A `stacklyd` this test started, stopped with SIGTERM and reaped on drop.
struct Daemon(Child);

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = Command::new("/bin/kill")
            .args(["-TERM", &self.0.id().to_string()])
            .status();
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if matches!(self.0.try_wait(), Ok(Some(_))) {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

// covers: AC-12, AC-13 (spec 0034)
#[test]
fn the_binary_empties_old_previews_and_logs_an_export_with_the_home_folder_as_tilde() {
    let ev = evidence!(
        covers = ["REQ-019", "NFR-016"],
        kind = Kind::Fixture,
        expected = "stacklyd with HOME inside the test root removes a preview left in cache/diagnostics at start, and diagnostics.export_bundle to a path under that HOME logs diagnostics_exported with the path written ~ while the reply keeps the absolute path",
    );
    let root = TestRoot::new().unwrap();
    let home = root.path().join("home");
    std::fs::create_dir(&home).unwrap();
    let stale = root.path().join("cache/diagnostics");
    std::fs::create_dir_all(&stale).unwrap();
    for folder in [root.path().join("cache"), stale.clone()] {
        std::fs::set_permissions(&folder, std::os::unix::fs::PermissionsExt::from_mode(0o700))
            .unwrap();
    }
    std::fs::write(stale.join("old.zip"), "old").unwrap();
    let paths = daemon_paths(&root);
    let stderr_path = root.path().join("stacklyd.stderr");
    let stderr = std::fs::File::create(&stderr_path).unwrap();
    let _daemon = Daemon(
        Command::new(STACKLYD)
            .env("STACKLY_ROOT", root.path())
            .env("HOME", &home)
            .env("STACKLY_LOG", "info")
            .env_remove("STACKLY_CATALOG_DIR")
            .env_remove("XPC_SERVICE_NAME")
            .stdout(Stdio::null())
            .stderr(stderr)
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(30);
    while !handshakes(&paths.socket) {
        assert!(
            Instant::now() < deadline,
            "stacklyd did not come up: {}",
            std::fs::read_to_string(&stderr_path).unwrap_or_default()
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        !stale.join("old.zip").exists(),
        "the start emptied the staging folder"
    );

    let target = home.join("out/bundle.zip");
    std::fs::create_dir(target.parent().unwrap()).unwrap();
    let rt = runtime();
    let (preview, exported) = rt.block_on(async {
        let (client, _) = client(&paths.socket).await;
        let Reply::DiagnosticsPreviewBundle(preview) =
            client.call(Call::DiagnosticsPreviewBundle).await.unwrap()
        else {
            panic!("not a preview reply");
        };
        let Reply::DiagnosticsExportBundle(exported) = client
            .call(Call::DiagnosticsExportBundle(ExportBundleParams {
                preview_id: preview.preview_id,
                destination: target.display().to_string(),
                replace: false,
            }))
            .await
            .unwrap()
        else {
            panic!("not an export reply");
        };
        (preview, exported)
    });
    assert_eq!(
        Path::new(&exported.path),
        target.canonicalize().unwrap(),
        "the reply keeps the absolute path"
    );
    assert!(
        root.path()
            .join(format!("cache/diagnostics/{}.zip", preview.preview_id))
            .exists()
    );

    let deadline = Instant::now() + Duration::from_secs(10);
    let line = loop {
        let mut text = String::new();
        std::fs::File::open(&stderr_path)
            .unwrap()
            .read_to_string(&mut text)
            .unwrap();
        if let Some(line) = text
            .lines()
            .find(|line| line.contains("diagnostics_exported"))
        {
            break line.to_owned();
        }
        assert!(
            Instant::now() < deadline,
            "no diagnostics_exported line: {text}"
        );
        std::thread::sleep(Duration::from_millis(20));
    };
    assert!(line.contains("\"path\":\"~/out/bundle.zip\""), "{line}");
    assert!(!line.contains(&home.display().to_string()), "{line}");

    let entries = stackly_core::diagnostics::testing::unzip(&std::fs::read(&target).unwrap());
    let system = String::from_utf8(entries["system.json"].clone()).unwrap();
    assert!(system.contains("\"data\""), "{system}");
    ev.pass();
}
