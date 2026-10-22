//! The rows the instance commands add to the exit code matrix of spec
//! 0008: `alreadyInState`, `needs_recovery` under `--wait`,
//! `resource.not_found`, an `instance.get` that fails after the wait, and
//! `cli.usage`.

use stackly_protocol::operation::{OperationKind, OperationState};
use stackly_protocol::{Call, Reply};
use stackly_testkit::TestRoot;
use stackly_testkit::evidence::{Kind, evidence};

use crate::common::scripted::{ScriptedDaemon, Step, error};
use crate::common::{dto, envelope, run};
use crate::data;

const ID: &str = "0192f3a5-0000-7000-8000-000000000001";

fn recovering() -> stackly_protocol::operation::OperationSummary {
    let mut summary = dto::ended(OperationState::NeedsRecovery, true, 3);
    summary.kind = OperationKind::StartInstance;
    summary.resource = Some(stackly_protocol::ResourceRef {
        kind: stackly_protocol::ResourceKind::Instance,
        id: ID.to_owned(),
    });
    summary
}

fn start_queued() -> Reply {
    Reply::InstanceStart(Box::new(data::accepted(
        data::operation(OperationKind::StartInstance, OperationState::Queued, 1),
        false,
    )))
}

/// Spec 0014 AC-6, AC-7.
#[test]
fn the_instance_rows_of_the_exit_code_matrix() {
    let ev = evidence!(
        covers = ["REQ-028"],
        kind = Kind::Fixture,
        expected = "alreadyInState exits 0, needs_recovery under --wait exits by the category of its error, resource.not_found exits 10, an instance.get that fails after the wait leaves the exit code to the operation, and cli.usage exits 2",
    );
    type Handler = Box<dyn Fn(&Call, crate::common::scripted::Ctx) -> Vec<Step> + Send + Sync>;
    let rows: Vec<(&str, Vec<&str>, i32, &str, Handler)> = vec![
        (
            "alreadyInState",
            vec!["instance", "start", ID],
            0,
            "",
            Box::new(data::scripted(
                Reply::InstanceStart(Box::new(data::already())),
                Vec::new(),
                Some(data::instance("")),
            )),
        ),
        (
            "needs_recovery under --wait",
            vec!["instance", "start", ID, "--wait"],
            7,
            "instance.start_failed",
            Box::new(data::scripted(
                start_queued(),
                vec![recovering()],
                Some(data::instance("failed")),
            )),
        ),
        (
            "resource.not_found",
            vec!["instance", "stop", ID],
            10,
            "resource.not_found",
            Box::new(|call: &Call, _| match call {
                Call::InstanceStop(_) => vec![Step::Fail(error(
                    "resource.not_found",
                    "not_found",
                    "no instance has this id",
                ))],
                _ => Vec::new(),
            }),
        ),
        (
            "instance.get fails after the wait",
            vec!["instance", "start", ID, "--wait"],
            7,
            "instance.start_failed",
            Box::new(data::scripted(start_queued(), vec![recovering()], None)),
        ),
        (
            "cli.usage",
            vec!["instance", "start", ID, "--idempotency-key", "a b"],
            2,
            "cli.usage",
            Box::new(|_: &Call, _| Vec::new()),
        ),
    ];
    for (row, args, code, error_code, handler) in rows {
        let root = TestRoot::new().expect("a test root is made");
        let _daemon = ScriptedDaemon::serve(&root, handler);
        let mut all = args.clone();
        all.push("--json");

        let json = run(root.path(), &all);

        assert_eq!(json.code, Some(code), "{row}: {json:?}");
        let value = envelope(&json);
        if error_code.is_empty() {
            assert_eq!(value["error"], serde_json::Value::Null, "{row}");
        } else {
            assert_eq!(value["error"]["code"], error_code, "{row}");
        }
    }
    ev.pass();
}
