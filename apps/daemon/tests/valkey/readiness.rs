//! Honest readiness of Valkey (spec 0040 AC-5): `not_ready` with the
//! `LOADING` message while a snapshot loads, and `not_ready` with
//! `WRONGPASS` while the port accepts but the readiness user is gone, then
//! `ready` again once it is back.

use std::process::Command;
use std::time::{Duration, Instant};

use serde_json::json;
use stackly_protocol::instance::InstanceReadinessState;
use stackly_testkit::TestRoot;
use stackly_testkit::evidence::{Kind, evidence};

use crate::harness::{
    READINESS_USER, Reply, Site, Valkey, copy_with_start_args, create_instance, port_of,
    stop_and_shutdown, stop_instance,
};

/// The keys the `LOADING` test writes: with a 2000 µs delay per key, the
/// load takes about 4 s.
const KEYS: usize = 2000;

/// Runs the readiness probe of the package by hand: its argv and env.
fn probe(site: &Site, port: u16) -> (bool, String) {
    let output = Command::new(site.bin("valkey-cli"))
        .args(["-e", "-h", "127.0.0.1", "-p", &port.to_string()])
        .args(["--user", READINESS_USER, "-t", "2", "PING"])
        .env_clear()
        .env("VALKEYCLI_AUTH", "nopass")
        .env("HOME", site.root.path())
        .output()
        .expect("valkey-cli runs");
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    (output.status.success(), format!("{stderr}{stdout}"))
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed Valkey 9.1.2 artifact in the runtime cache"]
async fn valkey_9_1_is_not_ready_with_loading_while_its_snapshot_loads() {
    let ev = evidence!(
        covers = ["REQ-038", "AC-038"],
        kind = Kind::Native,
        expected = "a Valkey rdb instance of a package copy with --key-load-delay 2000 and 2000 keys shows readiness not_ready with a LOADING detail while the snapshot loads, the probe argv run by hand exits 1 with LOADING, and then the instance is ready and the probe exits 0",
    );
    let copies = TestRoot::new().expect("a folder for the package copy");
    let dir = copy_with_start_args(
        &copies,
        &[
            "--key-load-delay",
            "2000",
            "--loading-process-events-interval-bytes",
            "1024",
        ],
    );
    let Some(site) = Site::open_package(&ev, &dir) else {
        return;
    };
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;
    let made = create_instance(&site, &api, "loading", json!({})).await;
    let running = api.start_ready(made.id).await;
    let port = port_of(&running);
    let mut valkey = Valkey::signed_in(port, &made.password);
    let version = valkey.info_server("valkey_version");
    site.subject(&ev, &version);
    for key in 0..KEYS {
        valkey.send(&["SET", &format!("key:{key}"), "value"]);
    }
    for _ in 0..KEYS {
        assert_eq!(valkey.read(), Reply::Simple("OK".to_owned()));
    }
    drop(valkey);
    stop_instance(&api, made.id).await;
    assert!(site.instance_paths(made.id).data.join("dump.rdb").is_file());

    api.start(made.id).await.expect("the start is accepted");
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut loading = None;
    let mut by_hand = None;
    let ready = loop {
        assert!(Instant::now() < deadline, "the instance never got ready");
        let summary = api.get(made.id).await.expect("the instance can be read");
        if summary.readiness.state == InstanceReadinessState::Ready {
            break summary;
        }
        if summary.readiness.state == InstanceReadinessState::NotReady
            && let Some(detail) = summary.readiness.detail.as_deref()
            && detail.starts_with("LOADING")
        {
            loading.get_or_insert_with(|| detail.to_owned());
            if by_hand.is_none() && summary.endpoints.iter().any(|e| e.port.is_some()) {
                let (ok, text) = probe(&site, port_of(&summary));
                if !ok && text.starts_with("LOADING") {
                    by_hand = Some(text);
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    };
    let loading = loading.expect("readiness showed a LOADING detail before ready");
    let by_hand = by_hand.expect("the probe run by hand exited 1 with LOADING");
    let (ok, text) = probe(&site, port_of(&ready));
    assert!(ok, "the probe exits 0 once loaded: {text}");
    stop_and_shutdown(&api, daemon, made.id).await;
    ev.pass_with(&format!(
        "not_ready with `{loading}`, the probe by hand said `{}`, then ready",
        by_hand.trim()
    ));
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed Valkey 9.1.2 artifact in the runtime cache"]
async fn valkey_9_1_is_not_ready_with_wrongpass_while_the_readiness_user_is_gone() {
    let ev = evidence!(
        covers = ["REQ-038", "AC-038"],
        kind = Kind::Native,
        expected = "after ACL DELUSER stackly-readiness the Valkey instance becomes not_ready with a WRONGPASS detail while its TCP port still accepts, and after ACL SETUSER stackly-readiness on nopass -@all +ping it is ready again",
    );
    let Some(site) = Site::open(&ev) else {
        return;
    };
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;
    let made = create_instance(&site, &api, "wrongpass", json!({})).await;
    let running = api.start_ready(made.id).await;
    let port = port_of(&running);
    let mut valkey = Valkey::signed_in(port, &made.password);
    let version = valkey.info_server("valkey_version");
    site.subject(&ev, &version);

    assert_eq!(
        valkey.cmd(&["ACL", "DELUSER", READINESS_USER]),
        Reply::Integer(1)
    );
    let not_ready = api
        .wait_instance(made.id, "not_ready", |summary| {
            summary.readiness.state == InstanceReadinessState::NotReady
        })
        .await;
    let detail = not_ready.readiness.detail.clone().unwrap_or_default();
    assert!(detail.contains("WRONGPASS"), "{detail}");
    assert!(
        std::net::TcpStream::connect(("127.0.0.1", port)).is_ok(),
        "the TCP port still accepts connections"
    );
    assert_eq!(
        valkey.cmd(&[
            "ACL",
            "SETUSER",
            READINESS_USER,
            "on",
            "nopass",
            "-@all",
            "+ping"
        ]),
        Reply::Simple("OK".to_owned())
    );
    api.wait_instance(made.id, "ready again", |summary| {
        summary.readiness.state == InstanceReadinessState::Ready
    })
    .await;
    stop_and_shutdown(&api, daemon, made.id).await;
    ev.pass_with(&format!("not_ready with `{detail}`, then ready again"));
}
