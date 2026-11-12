//! Connection, Open In, and the remedies of the Valkey package on a real
//! server (spec 0040 AC-18): `instance.get_detail` shows the `uri` and
//! `valkey` connections and the two Open In actions without the password,
//! the doctor gives `readiness_user` for a `WRONGPASS` readiness and
//! `aof_damaged` for a start that fails on a damaged append only file.

use std::io::Write;
use std::time::Duration;

use serde_json::json;
use stackly_protocol::doctor::{DoctorCheckId, DoctorFinding, DoctorParams};
use stackly_protocol::instance::{InstanceLifecycle, InstanceReadinessState};
use stackly_protocol::{Call, Reply};
use stackly_testkit::evidence::{Kind, evidence};
use uuid::Uuid;

use crate::harness::{
    READINESS_USER, Reply as Resp, Site, Valkey, create_instance, port_of, stop_and_shutdown,
    stop_instance,
};
use crate::support::Api;

/// The findings of `doctor.run` for the instance `id`.
async fn findings(api: &Api, id: Uuid) -> Vec<DoctorFinding> {
    let reply = api
        .client
        .call(Call::DoctorRun(DoctorParams {
            instance_id: Some(id),
            checks: vec![DoctorCheckId::InstanceFailures],
        }))
        .await
        .expect("doctor.run answers");
    let Reply::DoctorRun(report) = reply else {
        panic!("not a doctor reply: {reply:?}");
    };
    report
        .checks
        .into_iter()
        .flat_map(|check| check.findings)
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed Valkey 9.1.2 artifact in the runtime cache"]
async fn valkey_9_1_shows_its_connections_and_open_in_actions_without_the_password() {
    let ev = evidence!(
        covers = ["REQ-038"],
        kind = Kind::Native,
        expected = "instance.get_detail of a running Valkey instance shows the uri and valkey connections with absolute masked URIs and the tableplus and cli Open In actions with copySecret password and no password in a URL or an argument, and the doctor gives the readiness_user remedy while stackly-readiness is gone",
    );
    let Some(site) = Site::open(&ev) else {
        return;
    };
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;
    let made = create_instance(&site, &api, "connected", json!({})).await;
    let running = api.start_ready(made.id).await;
    let port = port_of(&running);
    let mut valkey = Valkey::signed_in(port, &made.password);
    let version = valkey.info_server("valkey_version");
    site.subject(&ev, &version);

    let detail = api.detail(made.id).await.expect("the detail reads");
    let names: Vec<&str> = detail
        .connections
        .iter()
        .map(|connection| connection.name.as_str())
        .collect();
    assert_eq!(names, ["uri", "valkey"]);
    for (connection, scheme) in detail.connections.iter().zip(["redis://", "valkey://"]) {
        let masked = connection.masked.as_deref().expect("a masked URI");
        assert!(masked.starts_with(scheme), "{masked}");
        assert!(masked.contains(&format!("127.0.0.1:{port}/0")), "{masked}");
        assert!(!masked.contains(&made.password), "{masked}");
    }
    let actions: Vec<&str> = detail
        .open_actions
        .iter()
        .map(|action| action.name.as_str())
        .collect();
    assert_eq!(actions, ["tableplus", "cli"]);
    for action in &detail.open_actions {
        assert!(action.copy_secret.is_some(), "{action:?}");
        let shown = format!("{:?} {:?}", action.url, action.command);
        assert!(!shown.contains(&made.password), "{shown}");
        assert!(!shown.contains("secret"), "{shown}");
    }

    assert_eq!(
        valkey.cmd(&["ACL", "DELUSER", READINESS_USER]),
        Resp::Integer(1)
    );
    api.wait_instance(made.id, "not_ready", |summary| {
        summary.readiness.state == InstanceReadinessState::NotReady
    })
    .await;
    let remedy = findings(&api, made.id)
        .await
        .into_iter()
        .find_map(|finding| finding.remedy)
        .expect("a finding with a remedy");
    assert_eq!(remedy.provider, "official/valkey");
    assert_eq!(remedy.name, "readiness_user");
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
        Resp::Simple("OK".to_owned())
    );
    stop_and_shutdown(&api, daemon, made.id).await;
    ev.pass_with("two connections, two Open In actions without the password, and readiness_user");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed Valkey 9.1.2 artifact in the runtime cache"]
async fn valkey_9_1_with_a_damaged_append_only_file_gets_the_aof_damaged_remedy() {
    let ev = evidence!(
        covers = ["REQ-038"],
        kind = Kind::Native,
        expected = "a Valkey aof instance whose incr file ends in bytes that are no command fails to start with Bad file format reading the append only file, the doctor gives the aof_damaged remedy, and the data folder is left as it was",
    );
    let Some(site) = Site::open(&ev) else {
        return;
    };
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;
    let made = create_instance(&site, &api, "damaged", json!({ "persistence": "aof" })).await;
    let running = api.start_ready(made.id).await;
    let mut valkey = Valkey::signed_in(port_of(&running), &made.password);
    let version = valkey.info_server("valkey_version");
    site.subject(&ev, &version);
    assert_eq!(
        valkey.cmd(&["SET", "kept", "yes"]),
        Resp::Simple("OK".to_owned())
    );
    drop(valkey);
    stop_instance(&api, made.id).await;

    let aof_dir = site.instance_paths(made.id).data.join("appendonlydir");
    let incr = std::fs::read_dir(&aof_dir)
        .expect("appendonlydir lists")
        .flatten()
        .map(|entry| entry.path())
        .find(|path| path.to_string_lossy().ends_with(".incr.aof"))
        .expect("an incr file");
    std::fs::OpenOptions::new()
        .append(true)
        .open(&incr)
        .and_then(|mut file| file.write_all(b"this is not a command\r\n"))
        .expect("the incr file can be damaged");
    let damaged = std::fs::read(&incr).expect("the incr file reads");

    let operation = api
        .start(made.id)
        .await
        .expect("the start is accepted")
        .accepted
        .expect("a start of a stopped instance begins an operation")
        .operation
        .id;
    api.wait_final(operation).await;
    let failed = api
        .wait_instance(made.id, "failed", |summary| {
            summary.lifecycle == InstanceLifecycle::Failed
        })
        .await;
    let found = findings(&api, made.id).await;
    let finding = found
        .iter()
        .find(|finding| finding.remedy.is_some())
        .unwrap_or_else(|| panic!("a finding with a remedy: {found:#?}"));
    let remedy = finding.remedy.as_ref().expect("a remedy");
    assert_eq!(remedy.name, "aof_damaged", "{found:#?}");
    assert!(
        finding
            .excerpt
            .iter()
            .any(|line| line.contains("Bad file format reading the append only file")),
        "{:?}",
        finding.excerpt
    );
    assert_eq!(
        std::fs::read(&incr).expect("the incr file reads"),
        damaged,
        "Stackly never repairs the file"
    );
    tokio::time::sleep(Duration::from_millis(100)).await;
    daemon.shutdown().await;
    ev.pass_with(&format!(
        "start failed ({:?}) and the doctor gave {}",
        failed.last_failure.map(|failure| failure.code),
        remedy.name
    ));
}
