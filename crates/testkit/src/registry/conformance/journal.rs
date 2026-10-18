//! The operation journal tables of spec 0007 (AC-2, AC-17): round trips of
//! journals, steps, created items, and lock keys, the check order of the
//! journal write ops and of `IdempotencyKeyTaken`, upserts that keep their
//! children, the four journal reads, and prune deleting journal rows with
//! their operation. Its prune deletes every finished operation older than
//! 30 days in the registry.

use std::time::Duration;

use jiff::Timestamp;
use stackly_domain::{
    BatchError, CreatedItem, CreatedRecord, CreatedSeq, CreatedState, FailureError, IdempotencyKey,
    InstanceId, JournalPayload, JournalStep, JournalView, LockKey, NewJournal, NewStep, Operation,
    OperationId, OperationJournal, OperationState, OperationTrigger, RecoveryCause, RecoveryHint,
    RegistryError, ResourceRef, RetentionPolicy, RuntimeId, StepSeq, WriteBatch, WriteOp,
};
use stackly_platform::Clock;

use super::{DAY, Suite, at, batch, domain, expect, expect_eq, fail, parse, rev, shifted, suffix};
use crate::registry::OperationBuilder;

pub(super) fn journal(suite: &Suite<'_>) {
    round_trip(suite);
    check_order(suite);
    upserts(suite);
    unique_rules(suite);
    idempotency_keys(suite);
    reads(suite);
    prune(suite);
}

/// A journal for `operation`, from a client request when `request`.
fn new_journal(check: &str, operation: OperationId, request: bool) -> OperationJournal {
    domain(
        check,
        OperationJournal::new(NewJournal {
            operation_id: operation,
            request: request.then(|| {
                (
                    parse(check, "instance.create"),
                    parse(check, &"ab".repeat(32)),
                )
            }),
            trigger: if request {
                OperationTrigger::user(Some(parse(check, "cli")), true)
            } else {
                OperationTrigger::background()
            },
            resumable: true,
            input: payload(check, r#"{"name":"orders-db"}"#),
            expected_revision: None,
        }),
    )
}

fn payload(check: &str, text: &str) -> JournalPayload {
    domain(
        check,
        JournalPayload::new(text, OperationJournal::INPUT_MAX),
    )
}

fn step_seq(check: &str, number: u16) -> StepSeq {
    domain(check, StepSeq::new(number))
}

fn created_seq(check: &str, number: u16) -> CreatedSeq {
    domain(check, CreatedSeq::new(number))
}

fn step(check: &str, number: u16, name: &str, at: Timestamp) -> JournalStep {
    JournalStep::begin(
        NewStep {
            seq: step_seq(check, number),
            name: parse(check, name),
            repeatable: number % 2 == 1,
            reversible: number < 3,
        },
        at,
    )
}

fn instance_record(check: &str, number: u16, step: u16, at: Timestamp) -> CreatedRecord {
    domain(
        check,
        CreatedRecord::intend(
            created_seq(check, number),
            step_seq(check, step),
            CreatedItem::InstanceTree(InstanceId::generate()),
            None,
            at,
        ),
    )
}

/// Stores `operation` with a journal and returns the journal.
fn store(suite: &Suite<'_>, check: &str, operation: &Operation, request: bool) -> OperationJournal {
    let journal = new_journal(check, operation.id(), request);
    suite.commit(
        check,
        vec![
            WriteOp::InsertOperation(operation.clone()),
            WriteOp::InsertJournal(journal.clone()),
        ],
    );
    journal
}

fn view(suite: &Suite<'_>, check: &str, id: OperationId) -> Option<JournalView> {
    suite.value(check, suite.registry.operation_journal(id))
}

/// Commits `batch`, which must fail at op `index` with an error `matches`
/// accepts, leaving the generation where it was.
fn refused(
    suite: &Suite<'_>,
    check: &str,
    batch: WriteBatch,
    index: usize,
    matches: impl FnOnce(&RegistryError) -> bool,
) {
    let before = suite.generation();
    match suite.registry.commit(batch) {
        Err(BatchError {
            op_index: Some(failed),
            error,
        }) if failed == index && matches(&error) => {}
        other => fail(
            check,
            &format!("expected a refusal at op {index}, got {other:?}"),
        ),
    }
    expect_eq(
        &format!("{check} (a failed batch leaves the generation)"),
        suite.generation(),
        before,
    );
}

fn round_trip(suite: &Suite<'_>) {
    const CHECK: &str = "journal_rows_round_trip";
    let t = suite.loaded_at;
    let operation = OperationBuilder::new()
        .state(OperationState::NeedsRecovery)
        .recovery_hint(RecoveryHint::Abandon)
        .build();
    let mut journal = new_journal(CHECK, operation.id(), true);
    let mut first = step(CHECK, 1, "download", t);
    domain(
        CHECK,
        first.finish(Some(payload(CHECK, r#"{"bytes":42}"#)), shifted(t, 1)),
    );
    let mut second = step(CHECK, 2, "extract", shifted(t, 2));
    domain(
        CHECK,
        second.fail(parse(CHECK, "runtime.extract_failed"), shifted(t, 3)),
    );
    let third = step(CHECK, 3, "commit", shifted(t, 4));
    let mut instance = instance_record(CHECK, 1, 1, t);
    domain(CHECK, instance.confirm());
    let mut custom = domain(
        CHECK,
        CreatedRecord::intend(
            created_seq(CHECK, 2),
            step_seq(CHECK, 2),
            CreatedItem::Custom {
                compensator: parse(CHECK, "supervisor_spawned_process"),
                key: parse(CHECK, &format!("pid:{}", suffix(operation.id()))),
            },
            Some(payload(CHECK, r#"{"pid":4242}"#)),
            shifted(t, 2),
        ),
    );
    domain(
        CHECK,
        custom.settle(
            CreatedState::NotOwned,
            Some(parse(CHECK, "created_by_other_operation")),
            shifted(t, 5),
        ),
    );
    let locks = vec![
        LockKey::Manager,
        LockKey::Runtime(RuntimeId::generate()),
        LockKey::InstanceName(parse(CHECK, &format!("db-{}", suffix(operation.id())))),
        domain(
            CHECK,
            LockKey::external_root(format!("/conformance/root-{}", suffix(operation.id()))),
        ),
    ];
    journal.set_waiting_on(Some(OperationId::generate()));
    journal.mark_point_of_no_return(shifted(t, 4));
    journal.mark_cancel_requested(shifted(t, 5));
    journal.mark_compensation_started(shifted(t, 6));
    journal.set_recovery_cause(Some(RecoveryCause::CompensationFailed));
    journal.set_failure_error(Some(domain(
        CHECK,
        FailureError::new(
            parse(CHECK, "operation.compensation_failed"),
            "undoing the runtime folder failed",
            vec![
                (parse(CHECK, "item"), "instance_tree".to_owned()),
                (parse(CHECK, "detail"), "permission_denied".to_owned()),
            ],
        ),
    )));
    domain(
        CHECK,
        journal.set_result(Some(payload(CHECK, r#"{"resources":[]}"#))),
    );

    suite.commit(
        "store an operation with every journal row",
        vec![
            WriteOp::InsertOperation(operation.clone()),
            WriteOp::InsertJournal(new_journal(CHECK, operation.id(), true)),
            WriteOp::UpsertStep {
                operation: operation.id(),
                step: third.clone(),
            },
            WriteOp::UpsertStep {
                operation: operation.id(),
                step: first.clone(),
            },
            WriteOp::UpsertStep {
                operation: operation.id(),
                step: second.clone(),
            },
            WriteOp::UpsertCreated {
                operation: operation.id(),
                record: custom.clone(),
            },
            WriteOp::UpsertCreated {
                operation: operation.id(),
                record: instance.clone(),
            },
            WriteOp::InsertLocks {
                operation: operation.id(),
                keys: locks.clone(),
            },
            WriteOp::UpdateJournal(journal.clone()),
        ],
    );

    let mut sorted_locks = locks;
    sorted_locks.sort_by_key(ToString::to_string);
    expect_eq(
        CHECK,
        view(suite, "read the journal", operation.id()),
        Some(JournalView::new(
            journal,
            vec![first, second, third],
            vec![instance, custom],
            sorted_locks,
        )),
    );
    expect_eq(
        &format!("{CHECK}: the recovery hint round trips"),
        suite
            .value(
                "read the operation",
                suite.registry.operation(operation.id()),
            )
            .and_then(|stored| stored.recovery_hint()),
        Some(RecoveryHint::Abandon),
    );
    expect_eq(
        &format!("{CHECK}: an operation without a journal has no journal view"),
        view(suite, "read a missing journal", OperationId::generate()),
        None,
    );
}

fn check_order(suite: &Suite<'_>) {
    let missing = OperationId::generate();
    let missing_ref = ResourceRef::Operation(missing);
    let t = suite.loaded_at;
    for (check, op) in [
        (
            "a journal for a missing operation is refused",
            WriteOp::InsertJournal(new_journal("make a journal", missing, false)),
        ),
        (
            "a step for a missing operation is refused",
            WriteOp::UpsertStep {
                operation: missing,
                step: step("make a step", 1, "download", t),
            },
        ),
        (
            "a created item for a missing operation is refused",
            WriteOp::UpsertCreated {
                operation: missing,
                record: instance_record("make a record", 1, 1, t),
            },
        ),
        (
            "locks for a missing operation are refused",
            WriteOp::InsertLocks {
                operation: missing,
                keys: vec![LockKey::Manager],
            },
        ),
        (
            "deleting the locks of a missing operation is refused",
            WriteOp::DeleteLocks { operation: missing },
        ),
    ] {
        suite.rejected(
            check,
            batch(vec![op]),
            at(
                0,
                RegistryError::MissingReference {
                    resource: missing_ref.clone(),
                },
            ),
        );
    }
    suite.rejected(
        "updating a journal that does not exist is refused",
        batch(vec![WriteOp::UpdateJournal(new_journal(
            "make a journal",
            missing,
            false,
        ))]),
        at(
            0,
            RegistryError::NotFound {
                resource: missing_ref,
            },
        ),
    );

    let operation = OperationBuilder::new().build();
    store(
        suite,
        "store an operation with a journal",
        &operation,
        false,
    );
    let id = operation.id();
    suite.rejected(
        "a second journal for an operation is refused",
        batch(vec![WriteOp::InsertJournal(new_journal(
            "make a journal",
            id,
            false,
        ))]),
        at(
            0,
            RegistryError::DuplicateResource {
                resource: ResourceRef::Operation(id),
            },
        ),
    );
    let seq = step_seq("make a step number", 7);
    suite.rejected(
        "a created item of a step the operation does not have is refused",
        batch(vec![WriteOp::UpsertCreated {
            operation: id,
            record: instance_record("make a record", 1, 7, t),
        }]),
        at(
            0,
            RegistryError::MissingReference {
                resource: ResourceRef::OperationStep(id, seq),
            },
        ),
    );
    #[cfg(unix)]
    {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        let not_utf8 = LockKey::ExternalRoot(OsStr::from_bytes(b"/opt/\xff").into());
        suite.rejected(
            "a lock key that is not UTF-8 is refused before the operation is looked up",
            batch(vec![WriteOp::InsertLocks {
                operation: OperationId::generate(),
                keys: vec![not_utf8],
            }]),
            at(0, RegistryError::NotUtf8Path { field: "lock_key" }),
        );
    }
}

fn upserts(suite: &Suite<'_>) {
    const CHECK: &str = "upserts_rewrite_a_row_and_keep_its_children";
    let t = suite.loaded_at;
    let operation = OperationBuilder::new().build();
    store(suite, "store an operation for upserts", &operation, false);
    let id = operation.id();
    let mut running = step(CHECK, 1, "download", t);
    let mut record = instance_record(CHECK, 1, 1, t);
    suite.commit(
        "store a step, a record, and locks",
        vec![
            WriteOp::UpsertStep {
                operation: id,
                step: running.clone(),
            },
            WriteOp::UpsertCreated {
                operation: id,
                record: record.clone(),
            },
            WriteOp::InsertLocks {
                operation: id,
                keys: vec![LockKey::Manager],
            },
        ],
    );
    domain(CHECK, running.finish(None, shifted(t, 1)));
    domain(CHECK, record.confirm());
    suite.commit(
        "rewrite the step and the record",
        vec![
            WriteOp::UpsertStep {
                operation: id,
                step: running.clone(),
            },
            WriteOp::UpsertCreated {
                operation: id,
                record: record.clone(),
            },
        ],
    );
    let read = view(suite, "read the journal after upserts", id)
        .unwrap_or_else(|| fail(CHECK, "the journal is gone"));
    expect_eq(
        CHECK,
        (read.steps, read.created),
        (vec![running], vec![record]),
    );

    let key = LockKey::Instance(InstanceId::generate());
    suite.commit(
        "replace the locks",
        vec![
            WriteOp::DeleteLocks { operation: id },
            WriteOp::InsertLocks {
                operation: id,
                keys: vec![key.clone()],
            },
        ],
    );
    expect_eq(
        "delete locks removes every key of the operation",
        view(suite, "read the locks", id).map(|read| read.locks),
        Some(vec![key]),
    );
}

fn unique_rules(suite: &Suite<'_>) {
    let t = suite.loaded_at;
    let operation = OperationBuilder::new().build();
    store(
        suite,
        "store an operation for unique rules",
        &operation,
        false,
    );
    let id = operation.id();
    let record = instance_record("make a record", 1, 1, t);
    suite.commit(
        "store a step, a record, and a lock",
        vec![
            WriteOp::UpsertStep {
                operation: id,
                step: step("make a step", 1, "download", t),
            },
            WriteOp::UpsertCreated {
                operation: id,
                record: record.clone(),
            },
            WriteOp::InsertLocks {
                operation: id,
                keys: vec![LockKey::Manager],
            },
        ],
    );
    let integrity = |error: &RegistryError| matches!(error, RegistryError::Integrity { .. });
    refused(
        suite,
        "two steps of one operation never share a name",
        batch(vec![WriteOp::UpsertStep {
            operation: id,
            step: step("make a step", 2, "download", t),
        }]),
        0,
        integrity,
    );
    let twin = domain(
        "make a twin record",
        CreatedRecord::intend(
            created_seq("make a number", 2),
            step_seq("make a number", 1),
            record.item().clone(),
            None,
            t,
        ),
    );
    refused(
        suite,
        "two created records of one operation never name the same item",
        batch(vec![WriteOp::UpsertCreated {
            operation: id,
            record: twin,
        }]),
        0,
        integrity,
    );
    refused(
        suite,
        "a lock key is recorded once per operation",
        batch(vec![WriteOp::InsertLocks {
            operation: id,
            keys: vec![LockKey::Manager],
        }]),
        0,
        integrity,
    );
}

fn idempotency_keys(suite: &Suite<'_>) {
    const CHECK: &str = "an_idempotency_key_is_bound_to_one_operation";
    let key = format!("req-{}", suffix(OperationId::generate()));
    let first = OperationBuilder::new().idempotency_key(&key).build();
    let journal = store(suite, "store an operation with a key", &first, true);
    let parsed: IdempotencyKey = parse(CHECK, &key);

    suite.rejected(
        CHECK,
        batch(vec![WriteOp::InsertOperation(
            OperationBuilder::new().idempotency_key(&key).build(),
        )]),
        at(
            0,
            RegistryError::IdempotencyKeyTaken {
                key: parsed.clone(),
                existing: first.id(),
            },
        ),
    );
    suite.rejected(
        "an existing ID is found before a taken key",
        batch(vec![WriteOp::InsertOperation(
            OperationBuilder::new()
                .with_id(first.id())
                .idempotency_key(&key)
                .build(),
        )]),
        at(
            0,
            RegistryError::DuplicateResource {
                resource: ResourceRef::Operation(first.id()),
            },
        ),
    );
    let mut started = first.clone();
    domain(
        CHECK,
        started.apply(stackly_domain::OperationEvent::Start, suite.loaded_at),
    );
    started.touch(suite.loaded_at);
    suite.commit(
        "an update keeps its own key",
        vec![WriteOp::UpdateOperation {
            operation: started.clone(),
            expected: rev(1),
        }],
    );
    expect_eq(
        "the key finds its operation and journal",
        suite.value(
            "read by key",
            suite.registry.operation_by_idempotency_key(&parsed),
        ),
        Some((started, journal)),
    );
    expect_eq(
        "an unknown key finds nothing",
        suite.value(
            "read by an unknown key",
            suite
                .registry
                .operation_by_idempotency_key(&parse(CHECK, "never-used-key")),
        ),
        None,
    );
}

fn reads(suite: &Suite<'_>) {
    let t = suite.clock.now();
    let states = [
        OperationState::Succeeded,
        OperationState::NeedsRecovery,
        OperationState::Queued,
        OperationState::Failed,
        OperationState::CancelRequested,
        OperationState::Cancelled,
        OperationState::Running,
    ];
    let operations: Vec<Operation> = states
        .iter()
        .enumerate()
        .map(|(index, state)| {
            let seconds = i64::try_from(index).unwrap_or(0);
            OperationBuilder::new()
                .state(*state)
                .created_at(shifted(t, -100 + seconds))
                .build()
        })
        .collect();
    let journals: Vec<OperationJournal> = operations
        .iter()
        .map(|operation| store(suite, "store an operation of each state", operation, false))
        .collect();
    let bare = OperationBuilder::new()
        .state(OperationState::Running)
        .created_at(shifted(t, -200))
        .build();
    suite.commit(
        "store an unfinished operation without a journal",
        vec![WriteOp::InsertOperation(bare.clone())],
    );

    let ids: Vec<OperationId> = operations.iter().map(Operation::id).collect();
    let mut asked = vec![ids[2], OperationId::generate(), bare.id(), ids[0]];
    asked.push(ids[6]);
    expect_eq(
        "operation_journals returns the journals of the IDs that have one, in order",
        suite.value(
            "read journals in a batch",
            suite.registry.operation_journals(&asked),
        ),
        vec![
            journals[2].clone(),
            journals[0].clone(),
            journals[6].clone(),
        ],
    );

    let unfinished: Vec<OperationId> = suite
        .value(
            "read unfinished operations",
            suite.registry.unfinished_operations(),
        )
        .into_iter()
        .map(|(operation, view)| {
            expect(
                "an unfinished operation comes with its own journal",
                view.journal.operation_id() == operation.id(),
                || {
                    format!(
                        "{} came with {}",
                        operation.id(),
                        view.journal.operation_id()
                    )
                },
            );
            operation.id()
        })
        .filter(|id| ids.contains(id) || *id == bare.id())
        .collect();
    expect_eq(
        "unfinished_operations returns queued, running, cancel_requested, and needs_recovery \
         operations with a journal, oldest first",
        unfinished,
        vec![ids[1], ids[2], ids[4], ids[6]],
    );
}

fn prune(suite: &Suite<'_>) {
    const CHECK: &str = "prune_deletes_journal_rows_with_their_operation";
    let now = suite.clock.now();
    let old = shifted(now, -40 * DAY);
    let key = format!("req-{}", suffix(OperationId::generate()));
    let finished = OperationBuilder::new()
        .state(OperationState::Succeeded)
        .idempotency_key(&key)
        .created_at(old)
        .build();
    let stuck = OperationBuilder::new()
        .state(OperationState::NeedsRecovery)
        .created_at(old)
        .build();
    for operation in [&finished, &stuck] {
        store(
            suite,
            "store an old operation with a journal",
            operation,
            true,
        );
        suite.commit(
            "store its step, record, and lock",
            vec![
                WriteOp::UpsertStep {
                    operation: operation.id(),
                    step: step(CHECK, 1, "download", old),
                },
                WriteOp::UpsertCreated {
                    operation: operation.id(),
                    record: instance_record(CHECK, 1, 1, old),
                },
                WriteOp::InsertLocks {
                    operation: operation.id(),
                    keys: vec![LockKey::Manager],
                },
            ],
        );
    }
    let policy = RetentionPolicy {
        operation_max_age: Duration::from_secs(30 * 24 * 60 * 60),
        operation_max_rows: u32::MAX,
        ..RetentionPolicy::DEFAULT
    };
    suite
        .registry
        .prune(&policy, now)
        .unwrap_or_else(|err| fail(CHECK, &err.to_string()));

    expect_eq(
        &format!("{CHECK}: a pruned operation leaves no journal"),
        view(suite, "read a pruned journal", finished.id()),
        None,
    );
    let kept = view(suite, "read a kept journal", stuck.id())
        .unwrap_or_else(|| fail(CHECK, "the needs_recovery operation lost its journal"));
    expect_eq(
        &format!("{CHECK}: a needs_recovery operation keeps every journal row"),
        (kept.steps.len(), kept.created.len(), kept.locks.len()),
        (1, 1, 1),
    );
    let reused = OperationBuilder::new().idempotency_key(&key).build();
    store(
        suite,
        "a pruned operation's key is free again",
        &reused,
        true,
    );
}
