//! `execute_reload` (spec 0010 AC-14): the config is rendered again, the
//! verified main process gets `SIGHUP`, readiness is checked, and the PID
//! never changes.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::process::Command;
use std::time::Duration;

use stackly_core::supervisor::codes;
use stackly_domain::{
    ConfigRevisionId, ConfigRevisionRecord, ConfigValuesJson, OperationState, Registry, SecretUser,
    WriteBatch, WriteOp,
};
use stackly_platform::{ExitStatus, ProviderSignal};

use crate::common::{LOOPBACK, Rig, Spec};

fn get(port: u16, path: &str) -> String {
    let mut stream = TcpStream::connect((LOOPBACK, port)).expect("the server accepts");
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"
    )
    .expect("the request is written");
    let mut reply = String::new();
    stream
        .read_to_string(&mut reply)
        .expect("the reply is read");
    reply
}

#[tokio::test]
async fn a_reload_renders_again_signals_hup_and_keeps_the_pid() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec::default());
    let op = rig.start(id).await;
    let spawn = rig.main_spawn(1).await;
    let port = rig.serve(&spawn);
    rig.wait_state(op, OperationState::Succeeded).await;
    assert!(get(port, "/config").contains("hello from stackly"));

    // A new config revision with another greeting.
    let active = rig
        .registry
        .active_config_revision(id)
        .unwrap()
        .value
        .unwrap();
    let mut values: serde_json::Value = serde_json::from_str(active.values.as_str()).unwrap();
    values["greeting"] = serde_json::Value::String("reloaded greeting".to_owned());
    let mut revision = ConfigRevisionRecord::first(
        ConfigRevisionId::generate(),
        id,
        ConfigValuesJson::new(values.to_string()).unwrap(),
        None,
        rig.fake_now(),
    );
    revision.previous = Some(active.id);
    rig.registry
        .commit(WriteBatch::new(vec![WriteOp::InsertConfigRevision(
            revision,
        )]))
        .unwrap();

    let reload = rig.reload(id).await;
    rig.wait_state(reload, OperationState::Succeeded).await;

    assert_eq!(rig.processes().signals(spawn.pid), [ProviderSignal::Hup]);
    // The fake process cannot deliver the signal, so a real one goes to the
    // dummy server that stands in for it.
    let server_pid = rig
        .servers
        .lock()
        .unwrap()
        .first()
        .map(std::process::Child::id)
        .unwrap();
    assert!(
        Command::new("kill")
            .args(["-HUP", &server_pid.to_string()])
            .status()
            .unwrap()
            .success()
    );
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        if get(port, "/config").contains("reloaded greeting") {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the greeting never changed"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(
        rig.sup.status(id).process.map(|process| process.pid),
        Some(spawn.pid),
        "the PID did not change"
    );
    assert_eq!(
        rig.processes().spawns().len(),
        1,
        "a reload never runs the process again"
    );
    let _ = (SecretUser::Instance(id), ExitStatus::Exited(0));
}

#[tokio::test]
async fn a_provider_without_reload_is_refused_without_any_signal() {
    let rig = Rig::without_reload().await;
    let id = rig.instance(&Spec::default());
    let op = rig.start(id).await;
    let spawn = rig.main_spawn(1).await;
    rig.serve(&spawn);
    rig.wait_state(op, OperationState::Succeeded).await;

    let reload = rig.reload(id).await;
    let detail = rig.wait_state(reload, OperationState::Failed).await;

    assert_eq!(
        Rig::failure_code(&detail),
        codes::CAPABILITY_UNSUPPORTED.to_string()
    );
    assert!(rig.processes().signals(spawn.pid).is_empty());
}

#[tokio::test]
async fn a_reload_of_an_instance_that_does_not_run_fails() {
    let rig = Rig::new().await;
    let id = rig.instance(&Spec::default());

    let reload = rig.reload(id).await;
    let detail = rig.wait_state(reload, OperationState::Failed).await;

    assert_eq!(Rig::failure_code(&detail), codes::RELOAD_FAILED.to_string());
}
