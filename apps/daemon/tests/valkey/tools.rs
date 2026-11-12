//! Only the programs of the bound runtime run (spec 0040 AC-12): decoy
//! `valkey-server`, `valkey-cli`, `redis-server`, and `redis-cli` first on
//! the daemon's `PATH` never run, and `INFO server` names the version and
//! the executable of the runtime.

use std::os::unix::fs::PermissionsExt;

use serde_json::json;
use stackly_testkit::evidence::{Kind, evidence};

use crate::harness::{Site, VERSION, Valkey, create_with, new_password, port_of, stop_instance};

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed Valkey 9.1.2 artifact in the runtime cache"]
async fn valkey_9_1_never_runs_a_decoy_valkey_or_redis_from_the_daemon_path() {
    let ev = evidence!(
        covers = ["REQ-038"],
        kind = Kind::Native,
        expected = "with decoy valkey-server, valkey-cli, redis-server, and redis-cli first on the daemon's PATH, create, start, readiness, and stop work, no decoy runs, and INFO server reports the artifact version and an executable inside the runtime root",
    );
    let Some(site) = Site::open(&ev) else {
        return;
    };
    let decoys = site.root.path().join("decoys");
    let marker = site.root.path().join("decoy-ran");
    std::fs::create_dir_all(&decoys).expect("the decoy folder can be made");
    for name in ["valkey-server", "valkey-cli", "redis-server", "redis-cli"] {
        let file = decoys.join(name);
        std::fs::write(
            &file,
            format!(
                "#!/bin/sh\necho \"$0 $*\" >> '{}'\nexit 1\n",
                marker.display()
            ),
        )
        .expect("a decoy can be written");
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755))
            .expect("a decoy can be made executable");
    }
    let path = format!("{}:/usr/bin:/bin", decoys.display());
    let daemon = site.binary(&[("PATH", &path)]);
    let api = site.binary_api(&daemon).await;

    let made = create_with(&site, &api, "decoyed", Some(&new_password()), json!({})).await;
    let running = api.start_ready(made.id).await;
    let mut valkey = Valkey::signed_in(port_of(&running), &made.password);
    let version = valkey.info_server("valkey_version");
    assert_eq!(version, VERSION);
    let executable = valkey.info_server("executable");
    assert!(
        std::path::Path::new(&executable).starts_with(&site.runtime_dir),
        "{executable} is inside {}",
        site.runtime_dir.display()
    );
    site.subject(&ev, &version);
    // Let several probe rounds run.
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    stop_instance(&api, made.id).await;
    daemon.stop();

    assert!(
        !marker.exists(),
        "a decoy ran: {}",
        std::fs::read_to_string(&marker).unwrap_or_default()
    );
    ev.pass_with(&format!(
        "Valkey {version} from {executable} ran with decoys first on PATH and none ran"
    ));
}
