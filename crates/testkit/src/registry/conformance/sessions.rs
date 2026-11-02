//! Login sessions (spec 0031): a session round trips, the registry holds
//! one open session at most, ending and reopening check existence, a prune
//! keeps the newest rows and the leases that still have a session, and an
//! instance has one startup lease per session.

use stackly_domain::{
    DaemonSession, LeaseFilter, RegistryError, ResourceRef, SessionEndReason, SessionId, WriteOp,
};

use super::{Suite, at, batch, domain, expect, expect_eq, parse, shifted};
use crate::registry::LeaseBuilder;

const BOOT: &str = "6f9619ff-8b86-4011-b42d-00c04fc964ff";

/// Runs every session scenario.
pub(super) fn sessions(suite: &Suite<'_>) {
    lifecycle(suite);
    one_open_session(suite);
    refusals(suite);
    startup_leases(suite);
    prune(suite);
}

fn session(suite: &Suite<'_>, audit: u32, seconds: i64) -> DaemonSession {
    DaemonSession::new(
        parse("make a session ID", &format!("{BOOT}/{audit}")),
        parse("make a boot ID", BOOT),
        audit,
        shifted(suite.loaded_at, seconds),
    )
    .expect("a session with audit session 1 or more is valid")
}

fn recent(suite: &Suite<'_>, check: &str, limit: u32) -> Vec<DaemonSession> {
    suite.value(check, suite.registry.recent_sessions(limit))
}

fn lifecycle(suite: &Suite<'_>) {
    const CHECK: &str = "a_session_round_trips_through_end_and_reopen";
    let first = session(suite, 1_001, 1_000);
    suite.commit(CHECK, vec![WriteOp::InsertSession(first.clone())]);
    expect_eq(CHECK, recent(suite, CHECK, 1), vec![first.clone()]);

    let ended_at = shifted(suite.loaded_at, 1_010);
    suite.commit(
        CHECK,
        vec![WriteOp::EndSession {
            id: first.id().clone(),
            at: ended_at,
            reason: SessionEndReason::Interrupted,
        }],
    );
    let stored = recent(suite, CHECK, 1).remove(0);
    expect_eq(
        CHECK,
        stored.ended(),
        Some((ended_at, SessionEndReason::Interrupted)),
    );

    suite.commit(
        CHECK,
        vec![WriteOp::ReopenSession {
            id: first.id().clone(),
        }],
    );
    expect_eq(CHECK, recent(suite, CHECK, 1), vec![first.clone()]);

    suite.commit(
        CHECK,
        vec![WriteOp::EndSession {
            id: first.id().clone(),
            at: ended_at,
            reason: SessionEndReason::Clean,
        }],
    );
}

fn one_open_session(suite: &Suite<'_>) {
    const CHECK: &str = "the_registry_holds_at_most_one_open_session";
    let second = session(suite, 1_002, 1_100);
    let third = session(suite, 1_003, 1_200);
    suite.commit(CHECK, vec![WriteOp::InsertSession(second.clone())]);
    match suite
        .registry
        .commit(batch(vec![WriteOp::InsertSession(third.clone())]))
    {
        Err(err) => expect(
            CHECK,
            err.op_index == Some(0) && matches!(err.error, RegistryError::Integrity { .. }),
            || format!("expected Integrity at op 0, got {err:?}"),
        ),
        Ok(_) => expect(CHECK, false, || {
            "a second open session committed".to_owned()
        }),
    }
    suite.commit(
        CHECK,
        vec![
            WriteOp::EndSession {
                id: second.id().clone(),
                at: shifted(suite.loaded_at, 1_150),
                reason: SessionEndReason::Unknown,
            },
            WriteOp::InsertSession(third.clone()),
        ],
    );
    let newest = recent(suite, CHECK, 2);
    expect_eq(
        "recent sessions are newest first",
        newest.iter().map(|s| s.id().clone()).collect::<Vec<_>>(),
        vec![third.id().clone(), second.id().clone()],
    );
    suite.commit(
        CHECK,
        vec![WriteOp::EndSession {
            id: third.id().clone(),
            at: shifted(suite.loaded_at, 1_250),
            reason: SessionEndReason::Clean,
        }],
    );
}

fn refusals(suite: &Suite<'_>) {
    const CHECK: &str = "session_ops_check_existence";
    let stored = recent(suite, CHECK, 1).remove(0);
    suite.rejected(
        CHECK,
        batch(vec![WriteOp::InsertSession(stored.clone())]),
        at(
            0,
            RegistryError::DuplicateResource {
                resource: ResourceRef::Session(stored.id().clone()),
            },
        ),
    );
    let unknown: SessionId = parse("make a session ID", "no-such-session");
    let missing = RegistryError::NotFound {
        resource: ResourceRef::Session(unknown.clone()),
    };
    suite.rejected(
        CHECK,
        batch(vec![WriteOp::ReopenSession {
            id: unknown.clone(),
        }]),
        at(0, missing.clone()),
    );
    suite.rejected(
        CHECK,
        batch(vec![WriteOp::EndSession {
            id: unknown,
            at: suite.loaded_at,
            reason: SessionEndReason::Clean,
        }]),
        at(0, missing),
    );
}

fn startup_leases(suite: &Suite<'_>) {
    const CHECK: &str = "an_instance_has_one_unreleased_startup_lease_per_session";
    let stack = suite.stack(CHECK);
    let instance = Suite::instance(&stack).build();
    suite.commit(CHECK, vec![WriteOp::InsertInstance(instance.clone())]);
    let id = format!("{BOOT}/1003");
    let first = LeaseBuilder::new(instance.id()).startup(&id);
    suite.commit(CHECK, vec![WriteOp::InsertLease(first.clone().build())]);
    let second = LeaseBuilder::new(instance.id()).startup(&id);
    suite.rejected(
        CHECK,
        batch(vec![WriteOp::InsertLease(second.clone().build())]),
        at(
            0,
            RegistryError::DuplicateResource {
                resource: ResourceRef::Lease(second.id()),
            },
        ),
    );
    suite.commit(
        CHECK,
        vec![WriteOp::UpdateLease(
            first.clone().released(shifted(suite.loaded_at, 1)).build(),
        )],
    );
    suite.commit(CHECK, vec![WriteOp::InsertLease(second.build())]);
    let other = LeaseBuilder::new(instance.id()).startup(&format!("{BOOT}/1004"));
    suite.commit(CHECK, vec![WriteOp::InsertLease(other.build())]);
    let leases = suite.value(
        CHECK,
        suite
            .registry
            .leases(instance.id(), LeaseFilter::Unreleased),
    );
    expect_eq(CHECK, leases.len(), 2);
}

fn prune(suite: &Suite<'_>) {
    const CHECK: &str = "prune_sessions_keeps_the_newest_rows_and_the_leases_of_kept_sessions";
    let stack = suite.stack(CHECK);
    let instance = Suite::instance(&stack).build();
    suite.commit(CHECK, vec![WriteOp::InsertInstance(instance.clone())]);
    let all = recent(suite, CHECK, 100);
    let newest = all[0].id().clone();
    let oldest = all[all.len() - 1].id().clone();
    let t = shifted(suite.loaded_at, 2_000);
    let old_lease = LeaseBuilder::new(instance.id())
        .startup(oldest.as_str())
        .released(t);
    let kept_lease = LeaseBuilder::new(instance.id())
        .startup(newest.as_str())
        .released(t);
    let active_old = LeaseBuilder::new(instance.id()).manual(Some(oldest.as_str()));
    let no_session = LeaseBuilder::new(instance.id()).manual(None).released(t);
    suite.commit(
        CHECK,
        vec![
            WriteOp::InsertLease(old_lease.clone().build()),
            WriteOp::InsertLease(kept_lease.clone().build()),
            WriteOp::InsertLease(active_old.clone().build()),
            WriteOp::InsertLease(no_session.clone().build()),
        ],
    );
    suite.commit(CHECK, vec![WriteOp::PruneSessions { keep: 1 }]);
    expect_eq(
        "one session row is left",
        recent(suite, CHECK, 100)
            .iter()
            .map(|s| s.id().clone())
            .collect::<Vec<_>>(),
        vec![newest],
    );
    let left = suite.value(
        CHECK,
        suite.registry.leases(instance.id(), LeaseFilter::All),
    );
    let ids: Vec<_> = left.iter().map(|lease| lease.id()).collect();
    expect(
        "the released lease of a pruned session goes",
        !ids.contains(&old_lease.id()),
        || format!("{ids:?}"),
    );
    for kept in [kept_lease.id(), active_old.id(), no_session.id()] {
        expect(CHECK, ids.contains(&kept), || {
            format!("lease {kept} was pruned")
        });
    }
    domain(CHECK, Ok::<(), String>(()));
}
