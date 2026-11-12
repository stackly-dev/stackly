//! The parameters Stackly owns on the `valkey-server` command line (spec
//! 0040 AC-10): no config file argument, so `CONFIG REWRITE` and `ACL SAVE`
//! are refused, `dir` and `dbfilename` are protected, a `CONFIG SET` lasts
//! only until the next start, and an `Auto` port is kept.

use serde_json::json;
use stackly_protocol::operation::OperationState;
use stackly_testkit::evidence::{Kind, evidence};

use crate::harness::{Reply, Site, Valkey, create_instance, pid_of, port_of, stop_and_shutdown};

/// Every file of the config folder with its content, sorted by name.
fn config_files(dir: &std::path::Path) -> Vec<(String, String)> {
    let mut files: Vec<(String, String)> = std::fs::read_dir(dir)
        .expect("the config folder lists")
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .map(|entry| {
            (
                entry.file_name().to_string_lossy().into_owned(),
                std::fs::read_to_string(entry.path()).expect("a config file reads"),
            )
        })
        .collect();
    files.sort();
    files
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed Valkey 9.1.2 artifact in the runtime cache"]
async fn valkey_9_1_refuses_to_move_what_stackly_owns_and_forgets_config_set_at_restart() {
    let ev = evidence!(
        covers = ["REQ-038"],
        kind = Kind::Native,
        expected = "CONFIG REWRITE is refused because the server runs without a config file, ACL SAVE is refused, CONFIG SET dir and dbfilename are refused as protected configs, CONFIG SET maxmemory 1mb is gone after a restart, the config folder is the same before and after, and the Auto port is used again",
    );
    let Some(site) = Site::open(&ev) else {
        return;
    };
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;
    let made = create_instance(&site, &api, "owned", json!({})).await;
    let running = api.start_ready(made.id).await;
    let port = port_of(&running);
    let paths = site.instance_paths(made.id);
    let before = config_files(&paths.config);
    let mut valkey = Valkey::signed_in(port, &made.password);
    let version = valkey.info_server("valkey_version");
    site.subject(&ev, &version);

    let rewrite = valkey.cmd(&["CONFIG", "REWRITE"]);
    assert!(
        rewrite
            .error()
            .is_some_and(|text| text.contains("The server is running without a config file")),
        "{rewrite:?}"
    );
    let save = valkey.cmd(&["ACL", "SAVE"]);
    assert!(save.error().is_some(), "ACL SAVE is refused: {save:?}");
    for (name, value) in [("dir", "/tmp"), ("dbfilename", "elsewhere.rdb")] {
        let reply = valkey.cmd(&["CONFIG", "SET", name, value]);
        assert!(
            reply
                .error()
                .is_some_and(|text| text.contains("can't set protected config")),
            "{name}: {reply:?}"
        );
    }
    assert_eq!(
        valkey.cmd(&["CONFIG", "SET", "maxmemory", "1mb"]),
        Reply::Simple("OK".to_owned())
    );
    assert_eq!(
        valkey.cmd(&["CONFIG", "GET", "maxmemory"]).config_value(),
        Some("1048576")
    );

    let accepted = api
        .restart(made.id)
        .await
        .expect("the restart is accepted")
        .accepted
        .expect("a restart starts an operation");
    api.wait_state(accepted.operation.id, OperationState::Succeeded)
        .await;
    let again = api
        .wait_instance(made.id, "ready after the restart", |summary| {
            summary.readiness.state == stackly_protocol::instance::InstanceReadinessState::Ready
                && summary
                    .process
                    .as_ref()
                    .is_some_and(|process| process.pid != pid_of(&running))
        })
        .await;
    assert_eq!(port_of(&again), port, "the Auto port is kept");
    let mut valkey = Valkey::signed_in(port, &made.password);
    assert_eq!(
        valkey.cmd(&["CONFIG", "GET", "maxmemory"]).config_value(),
        Some("0"),
        "the next start goes back to the settings"
    );
    assert_eq!(config_files(&paths.config), before, "the config folder");
    stop_and_shutdown(&api, daemon, made.id).await;
    ev.pass_with(&format!(
        "Valkey {version} refused CONFIG REWRITE, ACL SAVE, and protected configs, and forgot maxmemory 1mb at restart on port {port}"
    ));
}
