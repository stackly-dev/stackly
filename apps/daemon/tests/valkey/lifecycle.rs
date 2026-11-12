//! The lifecycle of a Valkey instance through the daemon (spec 0040 AC-13):
//! create with a generated password, start with readiness `not_ready`
//! before `ready` from the event stream, UTC log lines through
//! `instance.logs_read`, the instance lock on fd 3, adoption of the same
//! process by a daemon that comes back after `SIGKILL`, and a password
//! change through `secret.set` that applies at the next start.

use std::time::Duration;

use serde_json::json;
use stackly_protocol::events::SubscribeParams;
use stackly_protocol::instance::{InstanceLifecycle, InstanceReadinessState};
use stackly_protocol::logs::{LogEntryKind, LogsReadParams};
use stackly_protocol::operation::OperationState;
use stackly_protocol::secret::{
    SecretEffectKind, SecretInput, SecretMode, SecretText, SetSecretParams,
};
use stackly_protocol::{Call, Reply};
use stackly_testkit::evidence::{Kind, evidence};

use crate::harness::{
    Reply as Resp, Site, VERSION, Valkey, alive, assert_not_ready_before_ready, command_line,
    create_with, drain_readiness, eventually, new_password, output_of, pid_of, port_of,
    stop_and_shutdown, stop_instance,
};

/// The hour of a Valkey log line `pid:role dd Mon yyyy hh:mm:ss.mmm level
/// text`, if the line has that shape.
fn valkey_hour(line: &str) -> Option<u32> {
    let mut words = line.split_whitespace();
    let _pid_role = words.next()?;
    let _day = words.next()?;
    let _month = words.next()?;
    let _year = words.next()?;
    let time = words.next()?;
    time.split(':').next()?.parse().ok()
}

// covers: AC-13 (spec 0040)
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed Valkey 9.1.2 artifact in the runtime cache"]
async fn valkey_9_1_runs_its_lifecycle_through_the_daemon() {
    let ev = evidence!(
        covers = ["REQ-038", "AC-038"],
        kind = Kind::Native,
        expected = "a Valkey 9.1.2 instance created with a generated password starts, shows not_ready before ready in its events, writes UTC log lines that instance.logs_read returns, holds the instance lock on fd 3 with the instance paths in its argv, and stops",
    );
    let Some(site) = Site::open(&ev) else {
        return;
    };
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;
    let (_, mut events) = api
        .client
        .subscribe(SubscribeParams::default())
        .await
        .expect("the subscription is accepted");

    let made = create_with(&site, &api, "lifecycle", None, json!({})).await;
    assert!(made.password.len() >= 8, "a generated password");
    let created = api.get(made.id).await.expect("the instance can be read");
    assert_eq!(created.lifecycle, InstanceLifecycle::Stopped);
    assert_eq!(created.initialized, Some(true));

    let running = api.start_ready(made.id).await;
    assert_eq!(running.lifecycle, InstanceLifecycle::Running);
    assert_eq!(running.readiness.state, InstanceReadinessState::Ready);
    let seen = drain_readiness(&mut events, made.id).await;
    assert_not_ready_before_ready(&seen);

    let port = port_of(&running);
    let mut valkey = Valkey::signed_in(port, &made.password);
    let version = valkey.info_server("valkey_version");
    assert_eq!(version, VERSION);
    site.subject(&ev, &version);
    assert_eq!(
        valkey.cmd(&["SET", "sentinel", "here"]),
        Resp::Simple("OK".to_owned())
    );

    let pid = pid_of(&running);
    let paths = site.instance_paths(made.id);
    let fd3 = output_of(
        "/usr/sbin/lsof",
        &["-nP", "-a", "-p", &pid.to_string(), "-d", "3"],
    );
    assert!(fd3.contains("instance.lock"), "{fd3}");
    let args = command_line(pid);
    assert!(
        args.contains(&paths.config.join("valkey.conf").display().to_string()),
        "set-proc-title no keeps the argv: {args}"
    );

    let mut page = None;
    for _ in 0..100 {
        let reply = api
            .client
            .call(Call::InstanceLogsRead(LogsReadParams {
                instance: made.id,
                streams: Vec::new(),
                contains: Some("Ready to accept connections".to_owned()),
                position: None,
                limit: Some(100),
            }))
            .await
            .expect("the log reads");
        let Reply::InstanceLogsRead(found) = reply else {
            panic!("not a logs_read reply: {reply:?}");
        };
        if !found.entries.is_empty() {
            page = Some(found);
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let page = page.expect("the ready line reaches the log store");
    let entry = &page.entries[0];
    let LogEntryKind::Line(line) = &entry.kind else {
        panic!("a log line: {entry:?}");
    };
    let hour = valkey_hour(&line.text).unwrap_or_else(|| panic!("a Valkey line: {}", line.text));
    let at = entry.at.as_jiff().to_zoned(jiff::tz::TimeZone::UTC).hour();
    let at = u32::try_from(at).expect("an hour fits");
    assert!(
        hour == at || (hour + 1) % 24 == at,
        "the Valkey time is UTC: {} at {}",
        line.text,
        entry.at.as_jiff()
    );

    stop_instance(&api, made.id).await;
    eventually("the process to be gone", Duration::from_secs(10), || {
        !alive(pid)
    })
    .await;
    let stopped = api.get(made.id).await.expect("the instance can be read");
    assert_eq!(stopped.lifecycle, InstanceLifecycle::Stopped);
    assert!(stopped.last_failure.is_none(), "{stopped:#?}");
    daemon.shutdown().await;
    ev.pass_with(&format!(
        "Valkey {version} was not_ready then ready, logged `{}` in UTC, held instance.lock on fd 3, and stopped",
        line.text
    ));
}

// covers: AC-13 (spec 0040)
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed Valkey 9.1.2 artifact in the runtime cache"]
async fn valkey_9_1_is_adopted_by_a_daemon_that_comes_back_after_sigkill() {
    let ev = evidence!(
        covers = ["REQ-038"],
        kind = Kind::Native,
        expected = "after SIGKILL to a stacklyd whose Valkey instance is ready, the next stacklyd adopts the same process with the same PID and start time, and the instance stays ready",
    );
    let Some(site) = Site::open(&ev) else {
        return;
    };
    let daemon = site.binary(&[]);
    let api = site.binary_api(&daemon).await;
    // The binary daemon keeps secrets in the test keychain of the root, so
    // the test gives the password instead of reading a generated one.
    let made = create_with(&site, &api, "adopted", Some(&new_password()), json!({})).await;
    let running = api.start_ready(made.id).await;
    let before = running.process.clone().expect("a process runs");
    let mut valkey = Valkey::signed_in(port_of(&running), &made.password);
    let version = valkey.info_server("valkey_version");
    site.subject(&ev, &version);
    drop(api);
    daemon.kill();
    assert!(alive(before.pid), "the instance outlives the daemon");

    let daemon = site.binary(&[]);
    let api = site.binary_api(&daemon).await;
    let adopted = api
        .wait_instance(made.id, "adopted and ready", |summary| {
            summary.readiness.state == InstanceReadinessState::Ready
                && summary
                    .process
                    .as_ref()
                    .is_some_and(|process| process.adopted)
        })
        .await;
    let after = adopted.process.clone().expect("a process runs");
    assert_eq!(after.pid, before.pid);
    assert_eq!(after.started_at, before.started_at);
    assert_eq!(
        valkey.cmd(&["PING"]),
        Resp::Simple("PONG".to_owned()),
        "the same server answers on the same connection"
    );
    stop_instance(&api, made.id).await;
    daemon.stop();
    ev.pass_with(&format!(
        "PID {} adopted with the same start time after SIGKILL to stacklyd",
        after.pid
    ));
}

// covers: AC-13 (spec 0040)
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed Valkey 9.1.2 artifact in the runtime cache"]
async fn valkey_9_1_takes_a_new_password_from_secret_set_at_the_next_start() {
    let ev = evidence!(
        covers = ["REQ-038"],
        kind = Kind::Native,
        expected = "secret.set of the password of a running Valkey instance reports next_start; after a restart the old password is refused with WRONGPASS and the new one signs in",
    );
    let Some(site) = Site::open(&ev) else {
        return;
    };
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;
    let made = create_with(&site, &api, "rotated", None, json!({})).await;
    let running = api.start_ready(made.id).await;
    let mut valkey = Valkey::signed_in(port_of(&running), &made.password);
    site.subject(&ev, &valkey.info_server("valkey_version"));

    let fresh = new_password();
    let reply = api
        .client
        .call(Call::SecretSet(SetSecretParams {
            reference: format!("local:instance/{}/password", made.id),
            input: SecretInput::Value(SecretText::new(fresh.clone())),
            mode: SecretMode::Replace,
            expected_revision: None,
            confirm_stored_only: false,
        }))
        .await
        .expect("secret.set answers");
    let Reply::SecretSet(stored) = reply else {
        panic!("not a set reply: {reply:?}");
    };
    assert!(
        stored.effects.iter().any(
            |effect| effect.field == "password" && effect.effect == SecretEffectKind::NextStart
        ),
        "{stored:#?}"
    );
    let mut still = Valkey::connect(port_of(&running));
    assert_eq!(
        still.cmd(&["AUTH", &made.password]),
        Resp::Simple("OK".to_owned()),
        "the running server keeps the old password until the next start"
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
            summary.readiness.state == InstanceReadinessState::Ready
                && summary
                    .process
                    .as_ref()
                    .is_some_and(|process| process.pid != pid_of(&running))
        })
        .await;
    let port = port_of(&again);
    let mut old = Valkey::connect(port);
    let refused = old.cmd(&["AUTH", &made.password]);
    assert!(
        refused
            .error()
            .is_some_and(|text| text.starts_with("WRONGPASS")),
        "{refused:?}"
    );
    let mut new = Valkey::connect(port);
    assert_eq!(new.cmd(&["AUTH", &fresh]), Resp::Simple("OK".to_owned()));
    stop_and_shutdown(&api, daemon, made.id).await;
    ev.pass_with(
        "secret.set said next_start, the restart refused the old password and took the new one",
    );
}
