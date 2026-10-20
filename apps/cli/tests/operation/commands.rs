//! AC-7, AC-15, AC-3, AC-12: the operation commands send what their flags
//! say, write their text and `--json` result, and exit by the category of
//! what went wrong.

use std::sync::Arc;

use stackly_protocol::codes::operation;
use stackly_protocol::operation::{
    AbandonReply, CancelDisposition, CancelReply, CountedProgress, CreatedItemInfo, CreatedState,
    OperationDetail, OperationKind, OperationPage, OperationProgress, OperationState,
    OperationStepInfo, StepState,
};
use stackly_protocol::{Call, Count, ErrorBody, Reply, ResourceKind, ResourceRef, Timestamp};
use stackly_testkit::TestRoot;
use stackly_testkit::evidence::{Kind, evidence};
use uuid::Uuid;

use crate::common::pty::{PtyOptions, run_in_pty};
use crate::common::scripted::{ScriptedDaemon, Step, error};
use crate::common::{dto, envelope, golden, run};

const NEXT: &str = "2026-10-18T03:04:05.000000000Z_0192f3a0-7c1e-7b2a-9d4e-5f6a7b8c9d01";

fn page() -> OperationPage {
    let mut first = dto::summary(dto::OPERATION, OperationState::Running, 4);
    first.stage = Some("数据库迁移".to_owned());
    first.progress = OperationProgress::Counted(CountedProgress {
        done: Count::new(2).expect("2 is a safe count"),
        total: Count::new(5).expect("5 is a safe count"),
    });
    let mut second = dto::ended(OperationState::Failed, true, 9);
    second.id = Uuid::from_u128(0x0192_f3a0_7c1e_7b2a_9d4e_5f6a_7b8c_9d02);
    second.kind = OperationKind::InstallRuntime;
    second.resource = Some(ResourceRef {
        kind: ResourceKind::Manager,
        id: "self".to_owned(),
    });
    // About three minutes old, so the AGE column reads `3m ago` whenever
    // the test runs.
    let created =
        Timestamp::from_jiff(jiff::Timestamp::now() - jiff::SignedDuration::from_secs(190));
    first.created_at = created;
    second.created_at = created;
    OperationPage {
        items: vec![first, second],
        next: Some(NEXT.to_owned()),
    }
}

fn at(text: &str) -> Timestamp {
    text.parse().expect("a fixed time parses")
}

fn inspected() -> OperationDetail {
    let mut summary = dto::ended(OperationState::Failed, true, 7);
    summary.stage = Some("wait_ready".to_owned());
    summary.progress = OperationProgress::Counted(CountedProgress {
        done: Count::new(2).expect("2 is a safe count"),
        total: Count::new(3).expect("3 is a safe count"),
    });
    summary.started_at = Some(at("2026-10-18T03:04:06Z"));
    summary.updated_at = at("2026-10-18T03:05:00Z");
    summary.point_of_no_return_at = Some(at("2026-10-18T03:04:30Z"));
    OperationDetail {
        operation: summary,
        steps: vec![
            OperationStepInfo {
                seq: 1,
                name: "render_config".to_owned(),
                state: StepState::Done,
                repeatable: true,
                reversible: true,
                attempts: 1,
                started_at: at("2026-10-18T03:04:06Z"),
                finished_at: Some(at("2026-10-18T03:04:07Z")),
                error_code: None,
            },
            OperationStepInfo {
                seq: 2,
                name: "wait_ready".to_owned(),
                state: StepState::Failed,
                repeatable: true,
                reversible: false,
                attempts: 2,
                started_at: at("2026-10-18T03:04:07Z"),
                finished_at: Some(at("2026-10-18T03:05:00Z")),
                error_code: Some("instance.start_failed".to_owned()),
            },
        ],
        created: vec![CreatedItemInfo {
            kind: "dir".to_owned(),
            reference: "data/instances/pg".to_owned(),
            state: CreatedState::Kept,
            detail: None,
        }],
    }
}

/// A daemon that answers every operation method the way the tests below
/// expect, and keeps the logs.
fn daemon(root: &TestRoot) -> ScriptedDaemon {
    let page = Arc::new(page());
    ScriptedDaemon::serve(root, move |call, _| match call {
        Call::OperationList(params) if params.active_only => {
            vec![Step::Reply(Reply::OperationList(OperationPage {
                items: Vec::new(),
                next: None,
            }))]
        }
        Call::OperationList(_) => vec![Step::Reply(Reply::OperationList((*page).clone()))],
        Call::OperationGet(params) if params.operation_id == dto::OPERATION => {
            vec![Step::Reply(Reply::OperationGet(Box::new(inspected())))]
        }
        Call::OperationGet(_) | Call::OperationCancel(_) => vec![Step::Fail(error(
            "resource.not_found",
            "not_found",
            "no operation has this id",
        ))],
        _ => Vec::new(),
    })
}

fn id() -> String {
    dto::OPERATION.to_string()
}

#[test]
fn list_writes_a_table_with_a_footer_and_sends_its_flags_as_params() {
    let ev = evidence!(
        covers = ["REQ-028", "REQ-054", "AC-028"],
        kind = Kind::Fixture,
        expected = "operation list sends operation.list with the flags as params (default limit 50), writes an aligned table even with wide text, a footer hint that repeats the flags, and the page as {items, next} with --json",
    );
    let root = TestRoot::new().unwrap();
    let daemon = daemon(&root);

    let text = run(root.path(), &["operation", "list"]);
    let json = run(root.path(), &["operation", "list", "--json"]);
    let flagged = run(
        root.path(),
        &[
            "operation",
            "list",
            "--resource",
            &format!("instance:{}", dto::INSTANCE),
            "--limit",
            "20",
            "--before",
            NEXT,
            "-v",
        ],
    );
    let empty = run(root.path(), &["operation", "list", "--active"]);

    assert_eq!(text.code, Some(0), "{text:?}");
    golden::check("operation_list.txt", &text.stdout);
    assert_eq!(
        text.stderr,
        format!("hint: more operations: stackly operation list --before {NEXT}\n")
    );
    assert_eq!(json.code, Some(0));
    let value = envelope(&json);
    assert_eq!(value["command"], "operation list");
    assert_eq!(value["result"]["next"], NEXT);
    assert_eq!(value["result"]["items"][0]["stage"], "数据库迁移");
    assert_eq!(json.stderr, "", "no footer with --json");
    assert!(flagged.stderr.contains(&format!(
        "hint: more operations: stackly operation list --resource instance:{} --limit 20 --before {NEXT}\n",
        dto::INSTANCE
    )));
    assert!(
        !flagged
            .stderr
            .lines()
            .filter(|line| line.starts_with("->") || line.starts_with("<-"))
            .any(|line| line.contains(&dto::INSTANCE.to_string())),
        "-v never writes params: {}",
        flagged.stderr
    );
    assert_eq!(empty.code, Some(0));
    assert_eq!(empty.stdout, "No operations.\n");
    assert_eq!(empty.stderr, "");

    let params: Vec<serde_json::Value> = daemon
        .log()
        .into_iter()
        .filter(|logged| logged.method == "operation.list")
        .map(|logged| logged.raw["call"]["content"].clone())
        .collect();
    assert_eq!(params[0]["limit"], 50);
    assert_eq!(params[0]["activeOnly"], false);
    assert_eq!(params[0]["resource"], serde_json::Value::Null);
    assert_eq!(params[2]["limit"], 20);
    assert_eq!(params[2]["before"], NEXT);
    assert_eq!(
        params[2]["resource"],
        serde_json::json!({ "kind": "instance", "id": dto::INSTANCE.to_string() })
    );
    assert_eq!(params[3]["activeOnly"], true);
    ev.pass();
}

#[test]
fn list_refuses_bad_flags_before_sending_anything() {
    let root = TestRoot::new().unwrap();
    let daemon = daemon(&root);
    for args in [
        &["--limit", "0"][..],
        &["--limit", "201"],
        &[
            "--resource",
            "operation:0192f3a0-7c1e-7b2a-9d4e-5f6a7b8c9d0e",
        ],
        &["--resource", "instance:abc"],
        &["--before", "page-2"],
    ] {
        let mut all = vec!["operation", "list", "--json"];
        all.extend_from_slice(args);
        let refused = run(root.path(), &all);
        assert_eq!(refused.code, Some(2), "{args:?}");
        assert_eq!(envelope(&refused)["error"]["code"], "cli.usage");
    }
    let manager = run(root.path(), &["operation", "list", "--resource", "manager"]);
    assert_eq!(manager.code, Some(0));
    let methods = daemon.methods();
    assert_eq!(
        methods,
        ["daemon.handshake", "operation.list"],
        "{methods:?}"
    );
    assert_eq!(
        daemon.log()[1].raw["call"]["content"]["resource"],
        serde_json::json!({ "kind": "manager", "id": "self" })
    );
}

#[test]
fn inspect_shows_the_operation_steps_and_created_items_and_exits_0_for_a_failed_one() {
    let root = TestRoot::new().unwrap();
    let _daemon = daemon(&root);

    let text = run(root.path(), &["operation", "inspect", &id()]);
    let json = run(root.path(), &["operation", "inspect", &id(), "--json"]);
    let missing = run(
        root.path(),
        &[
            "operation",
            "inspect",
            "0192f3a0-7c1e-7b2a-9d4e-5f6a7b8c9d09",
            "--json",
        ],
    );
    let partial_id = run(root.path(), &["operation", "inspect", "0192f3a0"]);

    assert_eq!(text.code, Some(0), "{text:?}");
    golden::check("operation_inspect.txt", &text.stdout);
    assert_eq!(json.code, Some(0));
    let value = envelope(&json);
    assert_eq!(value["result"]["operation"]["state"], "failed");
    assert_eq!(
        value["result"]["steps"][1]["errorCode"],
        "instance.start_failed"
    );
    golden::check("operation_inspect.json", &json.stdout);
    assert_eq!(missing.code, Some(10));
    assert_eq!(envelope(&missing)["error"]["code"], "resource.not_found");
    assert_eq!(partial_id.code, Some(2));
}

#[test]
fn cancel_reports_its_disposition_and_exits_3_when_it_cannot_apply() {
    let ev = evidence!(
        covers = ["REQ-028", "REQ-054", "AC-054"],
        kind = Kind::Fixture,
        expected = "operation cancel exits 0 for requested, cancelled_before_start, and continuing_past_point_of_no_return and says so as it is, 3 with cli.cancel_not_applied for already_final, by the recovery error for recovery_required, and 10 for an unknown id; it takes no --yes",
    );
    for (disposition, state, code, error, line) in [
        (
            CancelDisposition::Requested,
            OperationState::CancelRequested,
            0,
            None,
            "cancel requested for operation",
        ),
        (
            CancelDisposition::CancelledBeforeStart,
            OperationState::Cancelled,
            0,
            None,
            "it had not started yet",
        ),
        (
            CancelDisposition::ContinuingPastPointOfNoReturn,
            OperationState::CancelRequested,
            0,
            None,
            "past its point of no return and runs to its end",
        ),
        (
            CancelDisposition::AlreadyFinal,
            OperationState::Succeeded,
            3,
            Some("cli.cancel_not_applied"),
            "already ended (state succeeded)",
        ),
        (
            CancelDisposition::RecoveryRequired,
            OperationState::NeedsRecovery,
            7,
            Some("instance.start_failed"),
            "needs recovery (state needs_recovery)",
        ),
    ] {
        let root = TestRoot::new().unwrap();
        let daemon = ScriptedDaemon::serve(&root, move |call, _| match call {
            Call::OperationCancel(_) => {
                vec![Step::Reply(Reply::OperationCancel(Box::new(CancelReply {
                    operation: dto::ended(state, true, 3),
                    disposition,
                })))]
            }
            _ => Vec::new(),
        });

        let text = run(root.path(), &["operation", "cancel", &id()]);
        let json = run(root.path(), &["operation", "cancel", &id(), "--json"]);

        assert_eq!(text.code, Some(code), "{disposition:?}: {text:?}");
        assert!(
            text.stdout.contains(line),
            "{disposition:?}: {}",
            text.stdout
        );
        assert_eq!(json.code, Some(code));
        let value = envelope(&json);
        assert_eq!(
            value["result"]["disposition"],
            serde_json::to_value(disposition).unwrap()
        );
        match error {
            Some(error) => assert_eq!(value["error"]["code"], error),
            None => assert_eq!(value["error"], serde_json::Value::Null),
        }
        if disposition == CancelDisposition::Requested {
            golden::check("operation_cancel.txt", &text.stdout);
            golden::check("operation_cancel.json", &json.stdout);
            assert_eq!(
                text.stderr,
                format!(
                    "hint: wait with: stackly operation inspect {} --wait\n",
                    id()
                )
            );
        }
        assert_eq!(
            daemon.methods(),
            [
                "daemon.handshake",
                "operation.cancel",
                "daemon.handshake",
                "operation.cancel"
            ]
        );
    }
    let root = TestRoot::new().unwrap();
    let _daemon = daemon(&root);
    let missing = run(root.path(), &["operation", "cancel", &id(), "--json"]);
    assert_eq!(missing.code, Some(10));
    let yes = run(root.path(), &["operation", "cancel", &id(), "--yes"]);
    assert_eq!(
        yes.code,
        Some(2),
        "cancel needs no confirmation, so --yes is refused"
    );
    ev.pass();
}

/// A daemon for resume and abandon that checks `expectedRevision` against
/// the revision its `operation.get` gave.
fn recovering(root: &TestRoot) -> ScriptedDaemon {
    ScriptedDaemon::serve(root, |call, _| match call {
        Call::OperationGet(_) => vec![Step::Reply(Reply::OperationGet(Box::new(dto::detail(
            dto::ended(OperationState::NeedsRecovery, true, 6),
        ))))],
        Call::OperationResume(params)
            if params.expected_revision.map(|revision| revision.get()) == Some(6) =>
        {
            vec![Step::Reply(Reply::OperationResume(Box::new(dto::summary(
                dto::OPERATION,
                OperationState::Queued,
                7,
            ))))]
        }
        Call::OperationAbandon(params) if params.expected_revision.get() == 6 => {
            vec![Step::Reply(Reply::OperationAbandon(Box::new(
                AbandonReply {
                    operation: dto::ended(OperationState::Failed, true, 7),
                    leftovers: vec![CreatedItemInfo {
                        kind: "dir".to_owned(),
                        reference: "data/instances/pg\u{1b}[2J".to_owned(),
                        state: CreatedState::Kept,
                        detail: Some("not empty".to_owned()),
                    }],
                },
            )))]
        }
        Call::OperationResume(_) | Call::OperationAbandon(_) => vec![Step::Fail(
            ErrorBody::new(
                &stackly_protocol::codes::resource::REVISION_MISMATCH,
                "stale",
            )
            .with_detail("expected", "?")
            .with_detail("actual", "6"),
        )],
        _ => Vec::new(),
    })
}

#[test]
fn resume_sends_the_revision_operation_get_gave() {
    let ev = evidence!(
        covers = ["REQ-054"],
        kind = Kind::Fixture,
        expected = "operation resume reads the revision with operation.get and sends it as expectedRevision, then shows the resumed summary",
    );
    let root = TestRoot::new().unwrap();
    let daemon = recovering(&root);

    let text = run(root.path(), &["operation", "resume", &id()]);
    let json = run(root.path(), &["operation", "resume", &id(), "--json"]);

    assert_eq!(text.code, Some(0), "{text:?}");
    assert_eq!(
        text.stdout,
        format!("resumed operation {} (state queued)\n", id())
    );
    let value = envelope(&json);
    golden::check("operation_resume.json", &json.stdout);
    assert_eq!(value["result"]["state"], "queued");
    assert_eq!(value["result"]["revision"], 7);
    let methods = daemon.methods();
    assert_eq!(
        &methods[..3],
        ["daemon.handshake", "operation.get", "operation.resume"]
    );
    let resume = daemon
        .log()
        .into_iter()
        .find(|logged| logged.method == "operation.resume")
        .unwrap();
    assert_eq!(resume.raw["call"]["content"]["expectedRevision"], 6);

    let refused_root = TestRoot::new().unwrap();
    let _refusing = ScriptedDaemon::serve(&refused_root, |call, _| match call {
        Call::OperationGet(_) => vec![Step::Reply(Reply::OperationGet(Box::new(dto::detail(
            dto::ended(OperationState::Failed, true, 2),
        ))))],
        _ => vec![Step::Fail(
            ErrorBody::new(&operation::NOT_RESUMABLE, "the operation cannot be resumed")
                .with_detail("operationId", dto::OPERATION.to_string())
                .with_detail("reason", "wrong_state"),
        )],
    });
    let refused = run(
        refused_root.path(),
        &["operation", "resume", &id(), "--json"],
    );
    assert_eq!(refused.code, Some(3));
    assert_eq!(
        envelope(&refused)["error"]["code"],
        "operation.not_resumable"
    );
    ev.pass();
}

#[test]
fn abandon_is_a_destructive_confirmation_and_shows_the_leftovers() {
    let ev = evidence!(
        covers = ["REQ-054", "REQ-028"],
        kind = Kind::Fixture,
        expected = "operation abandon without --yes noninteractive fails cli.confirmation_required before anything reaches the daemon; with --yes or a yes answer it sends the revision from operation.get and shows the leftovers in text and --json; any other answer exits 9",
    );
    let root = TestRoot::new().unwrap();
    let daemon = recovering(&root);

    let refused = run(root.path(), &["operation", "abandon", &id(), "--json"]);
    assert_eq!(refused.code, Some(2), "{refused:?}");
    let value = envelope(&refused);
    assert_eq!(value["error"]["code"], "cli.confirmation_required");
    assert_eq!(
        crate::common::detail(&value["error"], "flag").as_deref(),
        Some("--yes")
    );
    assert!(daemon.methods().is_empty(), "nothing reached the daemon");

    let text = run(root.path(), &["operation", "abandon", &id(), "--yes"]);
    let json = run(
        root.path(),
        &["operation", "abandon", &id(), "-y", "--json"],
    );

    assert_eq!(text.code, Some(0), "{text:?}");
    golden::check("operation_abandon.txt", &text.stdout);
    assert!(!text.stdout.contains('\u{1b}'));
    let value = envelope(&json);
    golden::check("operation_abandon.json", &json.stdout);
    assert_eq!(value["result"]["leftovers"][0]["state"], "kept");
    assert_eq!(value["result"]["operation"]["state"], "failed");
    let abandon = daemon
        .log()
        .into_iter()
        .find(|logged| logged.method == "operation.abandon")
        .unwrap();
    assert_eq!(abandon.raw["call"]["content"]["expectedRevision"], 6);

    let tty = PtyOptions {
        stdin_tty: true,
        input: Some("yes\n"),
        ..PtyOptions::default()
    };
    let confirmed = run_in_pty(root.path(), &["operation", "abandon", &id()], &tty);
    assert_eq!(confirmed.code, Some(0), "{confirmed:?}");
    assert!(
        confirmed
            .stderr
            .contains(&format!("abandon operation {}?", id())),
        "{}",
        confirmed.stderr
    );
    assert!(confirmed.stderr.contains("[y/N] "));
    let declined = run_in_pty(
        root.path(),
        &["operation", "abandon", &id()],
        &PtyOptions {
            input: Some("\n"),
            ..tty.clone()
        },
    );
    assert_eq!(declined.code, Some(9), "{declined:?}");
    assert!(declined.stderr.contains("code: cli.confirmation_declined"));
    ev.pass();
}

#[test]
fn verbose_names_the_method_and_request_id_but_never_the_params_it_sent() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "AC-13: -v on operation list writes `-> operation.list <requestId>` and `<- ok <ms> ms` and never the cursor or the resource id the params carried, also together with -q",
    );
    let root = TestRoot::new().unwrap();
    let daemon = daemon(&root);
    let resource = format!("instance:{}", dto::INSTANCE);

    for quiet in [false, true] {
        let mut args = vec![
            "operation",
            "list",
            "--before",
            NEXT,
            "--resource",
            &resource,
            "-v",
        ];
        if quiet {
            args.push("-q");
        }
        let listed = run(root.path(), &args);
        assert_eq!(listed.code, Some(0), "{listed:?}");
        let list_id = daemon
            .log()
            .into_iter()
            .rev()
            .find(|logged| logged.method == "operation.list")
            .map(|logged| logged.request_id)
            .expect("the daemon saw operation.list");
        let lines: Vec<&str> = listed.stderr.lines().collect();
        assert!(
            lines.contains(&format!("-> operation.list {list_id}").as_str()),
            "quiet {quiet}: {}",
            listed.stderr
        );
        assert!(
            lines
                .iter()
                .any(|line| line.starts_with("<- ok ") && line.ends_with(" ms")),
            "quiet {quiet}: {}",
            listed.stderr
        );
        // The page footer is the command's own hint, not a -v line.
        for line in lines
            .iter()
            .filter(|line| line.starts_with("-> ") || line.starts_with("<- "))
        {
            for params in [NEXT, dto::INSTANCE.to_string().as_str()] {
                assert!(!line.contains(params), "-v leaked {params}: {line}");
            }
        }
    }
    ev.pass();
}

#[test]
fn a_timeout_without_wait_or_out_of_range_is_refused_before_connecting() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "AC-8: --timeout without --wait, below one second, above 168 hours, or not a duration exits 2 with cli.usage and nothing reaches the daemon, for inspect and for cancel",
    );
    let root = TestRoot::new().unwrap();
    let daemon = daemon(&root);
    let id = id();

    for verb in ["inspect", "cancel"] {
        for args in [
            &["--timeout", "5s"][..],
            &["--wait", "--timeout", "0"],
            &["--wait", "--timeout", "0s"],
            &["--wait", "--timeout", "169h"],
            &["--wait", "--timeout", "soon"],
            &["--wait", "--timeout", "-5s"],
        ] {
            let mut all = vec!["operation", verb, id.as_str(), "--json"];
            all.extend_from_slice(args);
            let refused = run(root.path(), &all);
            assert_eq!(refused.code, Some(2), "{verb} {args:?}: {refused:?}");
            let value = envelope(&refused);
            assert_eq!(value["error"]["code"], "cli.usage", "{verb} {args:?}");
            assert_eq!(value["command"], "", "{verb} {args:?}");
        }
    }
    assert!(daemon.methods().is_empty(), "{:?}", daemon.methods());
    ev.pass();
}

#[test]
fn an_abandon_answered_no_or_cut_off_exits_9_and_sends_nothing() {
    let ev = evidence!(
        covers = ["REQ-054", "REQ-028"],
        kind = Kind::Fixture,
        expected = "AC-5, AC-15: on a terminal, an abandon answered n, anything else, or end of input fails cli.confirmation_declined with exit 9 and operation.abandon never reaches the daemon; only y or yes in any case goes on",
    );
    let root = TestRoot::new().unwrap();
    let daemon = recovering(&root);
    let tty = PtyOptions {
        stdin_tty: true,
        ..PtyOptions::default()
    };

    for input in ["n\n", "maybe\n", "\u{4}"] {
        let declined = run_in_pty(
            root.path(),
            &["operation", "abandon", &id()],
            &PtyOptions {
                input: Some(input),
                ..tty.clone()
            },
        );
        assert_eq!(declined.code, Some(9), "{input:?}: {declined:?}");
        assert!(
            declined.stderr.contains("code: cli.confirmation_declined"),
            "{input:?}: {}",
            declined.stderr
        );
        assert_eq!(declined.stdout, "", "{input:?}");
    }
    assert!(
        !daemon
            .methods()
            .iter()
            .any(|method| method == "operation.abandon"),
        "{:?}",
        daemon.methods()
    );

    let upper = run_in_pty(
        root.path(),
        &["operation", "abandon", &id()],
        &PtyOptions {
            input: Some("YES\n"),
            ..tty
        },
    );
    assert_eq!(upper.code, Some(0), "{upper:?}");
    ev.pass();
}
