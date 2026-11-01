//! `instance create` against a real `stacklyd` with no catalog (spec 0020
//! AC-18, update 2026-11-01): the form comes from the pinned package that
//! `--provider` and `--revision` name, so `--set` reads as the kinds of that
//! package, and a revision that is not pinned ends at the form.

use serde_json::json;
use stackly_protocol::instance::GetDuplicateFormParams;
use stackly_protocol::{Call, Reply};
use stackly_testkit::evidence::{Kind, evidence};
use stackly_testkit::{ReapGuard, TestRoot};

use crate::common::stacklyd::start_daemon;
use crate::common::{envelope, paths, run};
use crate::duplicate_real::block_on;
use crate::stacklyd::seed;

/// Spec 0020 AC-18.
#[test]
fn without_a_catalog_set_values_read_as_the_kinds_of_the_pinned_package() {
    let ev = evidence!(
        covers = ["REQ-029"],
        kind = Kind::Fixture,
        expected = "with no catalog, create --provider testing/fake --revision 0.3.0 --set ignoreTerm=true --set readyDelayMs=4000 succeeds with a JSON boolean and number in the active settings, and an unpinned revision exits with instance.provider_not_found from the form without an instance.create",
    );
    let root = TestRoot::new().expect("a test root");
    let runtime = seed(&root);
    let _reap = ReapGuard::new(root.path(), &paths(&root).runtime);
    let daemon = start_daemon(&root);
    let create = |name: &str, revision: &str| {
        run(
            root.path(),
            &[
                "instance",
                "create",
                name,
                "--runtime",
                &runtime,
                "--provider",
                "testing/fake",
                "--revision",
                revision,
                "--set",
                "ignoreTerm=true",
                "--set",
                "readyDelayMs=4000",
                "--wait",
                "--timeout",
                "60s",
                "--json",
            ],
        )
    };

    let created = create("typed", "0.3.0");
    assert_eq!(created.code, Some(0), "{created:?}");
    let id = envelope(&created)["result"]["instance"]["id"]
        .as_str()
        .expect("the created instance has an ID")
        .to_owned();
    let settings = block_on(&paths(&root).socket, async move |client| {
        match client
            .call(Call::InstanceGetDuplicateForm(GetDuplicateFormParams {
                instance_id: id.parse().expect("an instance id"),
            }))
            .await
            .expect("the duplicate form is built")
        {
            Reply::InstanceGetDuplicateForm(form) => form.draft.settings,
            other => panic!("not a duplicate form: {other:?}"),
        }
    });
    let settings = settings.as_map();
    assert_eq!(
        settings.get("ignoreTerm"),
        Some(&json!(true)),
        "{settings:?}"
    );
    assert_eq!(
        settings.get("readyDelayMs"),
        Some(&json!(4000)),
        "{settings:?}"
    );

    let missing = create("missing", "9.9.9");
    assert_ne!(missing.code, Some(0), "{missing:?}");
    assert_eq!(
        envelope(&missing)["error"]["code"],
        "instance.provider_not_found",
        "{missing:?}"
    );
    let listed = run(root.path(), &["instance", "list", "--json"]);
    let names: Vec<_> = envelope(&listed)["result"]["items"]
        .as_array()
        .expect("the list has items")
        .iter()
        .map(|item| item["name"].clone())
        .collect();
    assert_eq!(
        names,
        [json!("typed")],
        "no create ran for the refused form"
    );
    assert_eq!(daemon.stop().unwrap().code(), Some(0));
    ev.pass();
}
