//! Two ClickHouse instances of one runtime side by side (spec 0043 AC-8,
//! AC-037): a `default.events` table of the same name in both, one row
//! each, a restart of one after the other, and every instance still reading
//! only its own row on its own ports, with the listener audit of AC-4 on
//! both at once. Stopping the first leaves the second as it was.

use serde_json::json;
use stackly_protocol::instance::InstanceReadinessState;
use stackly_protocol::operation::OperationState;
use stackly_testkit::evidence::{Kind, evidence};
use uuid::Uuid;

use crate::harness::{
    Made, Site, alive, assert_two_listeners, create_instance, pid_of, port_of, stop_and_shutdown,
    stop_instance,
};
use crate::support::Api;

/// Restarts the instance and waits until a new process is ready.
async fn restart_ready(api: &Api, id: Uuid) -> stackly_protocol::instance::InstanceSummary {
    let before = pid_of(&api.get(id).await.expect("the instance can be read"));
    let accepted = api
        .restart(id)
        .await
        .expect("the restart is accepted")
        .accepted
        .expect("a restart starts an operation");
    api.wait_state(accepted.operation.id, OperationState::Succeeded)
        .await;
    api.wait_instance(id, "ready after the restart", |summary| {
        summary.readiness.state == InstanceReadinessState::Ready
            && summary
                .process
                .as_ref()
                .is_some_and(|process| process.pid != before)
    })
    .await
}

/// The rows of `default.events` of the instance, as `id\tv` lines.
fn events(site: &Site, made: &Made, native: u16) -> String {
    site.sql_ok(
        native,
        "default",
        Some(&made.password),
        "SELECT * FROM default.events ORDER BY id",
    )
}

// covers: AC-8, AC-4 (spec 0043)
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed ClickHouse 26.8.30.8 artifact in the runtime cache"]
async fn two_clickhouse_instances_keep_their_own_events_table() {
    let ev = evidence!(
        covers = ["REQ-037", "AC-037"],
        kind = Kind::Native,
        expected = "two ClickHouse 26.8.30.8 instances of one runtime each create default.events and insert their own row, are restarted one after the other, and then each reads only its own row on its own two loopback ports, both listening on exactly their registry ports at the same time; stopping the first leaves the second running with the same PID and row",
    );
    let Some(site) = Site::open(&ev) else {
        return;
    };
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;
    let a = create_instance(&site, &api, "events-a", json!({})).await;
    let b = create_instance(&site, &api, "events-b", json!({})).await;
    let running_a = api.start_ready(a.id).await;
    let running_b = api.start_ready(b.id).await;
    site.subject(&ev, &site.version(port_of(&running_a, "http"), &a.password));

    for (made, running, row) in [
        (&a, &running_a, "(1, 'alpha')"),
        (&b, &running_b, "(2, 'beta')"),
    ] {
        let native = port_of(running, "native");
        site.exec(
            native,
            "default",
            Some(&made.password),
            "CREATE TABLE default.events (id UInt64, v String) ENGINE = MergeTree ORDER BY id",
        );
        site.exec(
            native,
            "default",
            Some(&made.password),
            &format!("INSERT INTO default.events VALUES {row}"),
        );
    }
    assert!(
        !site
            .sql(
                port_of(&running_a, "native"),
                "default",
                Some(&b.password),
                "SELECT 1"
            )
            .ok(),
        "the password of b does not open a"
    );

    let after_a = restart_ready(&api, a.id).await;
    let after_b = restart_ready(&api, b.id).await;

    let mut ports = Vec::new();
    for (made, summary, row) in [(&a, &after_a, "1\talpha"), (&b, &after_b, "2\tbeta")] {
        let http = port_of(summary, "http");
        let native = port_of(summary, "native");
        assert_eq!(events(&site, made, native), row, "{}", made.id);
        assert_two_listeners(pid_of(summary), http, native);
        assert_eq!(
            site.sql_ok(
                native,
                "default",
                Some(&made.password),
                "SELECT getMacro('stackly_instance_id')"
            ),
            made.id.to_string()
        );
        ports.extend([http, native]);
    }
    for (owner, other, summary) in [(&a, &b, &after_b), (&b, &a, &after_a)] {
        assert!(
            !site
                .sql(
                    port_of(summary, "native"),
                    "default",
                    Some(&owner.password),
                    "SELECT 1"
                )
                .ok(),
            "after both restarts the password of {} does not open {}",
            owner.id,
            other.id
        );
    }
    site.exec(
        port_of(&after_b, "native"),
        "default",
        Some(&b.password),
        "INSERT INTO default.events VALUES (3, 'gamma')",
    );
    assert_eq!(
        events(&site, &a, port_of(&after_a, "native")),
        "1\talpha",
        "a row written to b after both restarts stays in b"
    );
    site.exec(
        port_of(&after_a, "native"),
        "default",
        Some(&a.password),
        "INSERT INTO default.events VALUES (4, 'delta')",
    );
    assert_eq!(
        events(&site, &b, port_of(&after_b, "native")),
        "2\tbeta\n3\tgamma",
        "a row written to a after both restarts stays in a"
    );
    let mut unique = ports.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), 4, "four different ports: {ports:?}");
    let paths_a = site.instance_paths(a.id);
    let paths_b = site.instance_paths(b.id);
    assert_ne!(paths_a.data, paths_b.data);

    let pid_a = pid_of(&after_a);
    let pid_b = pid_of(&after_b);
    stop_instance(&api, a.id).await;
    crate::harness::eventually(
        "the first server to be gone",
        std::time::Duration::from_secs(60),
        || !alive(pid_a),
    )
    .await;
    let still = api.get(b.id).await.expect("the instance can be read");
    assert_eq!(
        pid_of(&still),
        pid_b,
        "the second instance keeps its process"
    );
    assert_eq!(still.readiness.state, InstanceReadinessState::Ready);
    assert_eq!(
        events(&site, &b, port_of(&still, "native")),
        "2\tbeta\n3\tgamma"
    );
    assert_two_listeners(pid_b, port_of(&still, "http"), port_of(&still, "native"));

    stop_and_shutdown(&api, daemon, b.id).await;
    ev.pass_with(&format!(
        "a read only (1, alpha) and b only (2, beta) after both restarts, neither password opened the other, later rows stayed in their own instance, on ports {ports:?}; b kept PID {pid_b} when a stopped"
    ));
}
