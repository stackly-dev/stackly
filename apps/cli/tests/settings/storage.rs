//! `stackly storage locations` and `set-location` (spec 0032 AC-19).

use serde_json::json;
use stackly_protocol::settings::LocationList;
use stackly_protocol::{Call, ErrorBody, Reply};
use stackly_testkit::TestRoot;

use crate::common::scripted::{ScriptedDaemon, Step};
use crate::common::{envelope, run};
use crate::{location, view};

fn refusal(rule: &str, message: &str, path: &str) -> ErrorBody {
    serde_json::from_value(json!({
        "code": "storage.location_invalid",
        "category": "invalid_input",
        "message": message,
        "retryable": false,
        "resource": null,
        "fields": [],
        "hint": null,
        "details": [
            { "key": "rule", "value": rule },
            { "key": "path", "value": path }
        ],
        "operationId": null,
        "rule": null,
        "items": []
    }))
    .expect("an error body")
}

#[test]
fn locations_lists_every_kind_with_its_use_count_and_reachability() {
    // covers: AC-19
    let root = TestRoot::new().expect("a test root is made");
    let _daemon = ScriptedDaemon::serve(&root, |call, _| match call {
        Call::StorageListLocations => {
            vec![Step::Reply(Reply::StorageListLocations(LocationList {
                items: vec![
                    location(
                        "00000000-0000-7000-8000-000000000001",
                        "data",
                        "/Users/dev/Library/Application Support/Stackly",
                        true,
                        2,
                    ),
                    location(
                        "0192f3a9-0000-7000-8000-000000000001",
                        "data",
                        "/Volumes/Work/Stackly",
                        false,
                        1,
                    ),
                ],
            }))]
        }
        _ => Vec::new(),
    });

    let text = run(root.path(), &["storage", "locations"]);
    let json = run(root.path(), &["storage", "locations", "--json"]);

    assert_eq!(text.code, Some(0), "{text:?}");
    assert!(text.stdout.contains("KIND"), "{}", text.stdout);
    assert!(
        text.stdout.contains("/Volumes/Work/Stackly"),
        "{}",
        text.stdout
    );
    let value = envelope(&json);
    assert_eq!(value["command"], "storage locations");
    assert_eq!(value["result"]["items"][1]["instanceCount"], 1);
}

#[test]
fn set_location_sends_an_absolute_path_with_the_revision_and_a_key() {
    // covers: AC-19
    let root = TestRoot::new().expect("a test root is made");
    let daemon = ScriptedDaemon::serve(&root, |call, _| match call {
        Call::SettingsGet => vec![Step::Reply(Reply::SettingsGet(Box::new(view(
            3, "manual", "stable", false,
        ))))],
        Call::StorageSetLocation(_) => {
            vec![Step::Reply(Reply::StorageSetLocation(Box::new(location(
                "0192f3a9-0000-7000-8000-000000000001",
                "data",
                "/Volumes/Work/Stackly",
                false,
                0,
            ))))]
        }
        _ => Vec::new(),
    });

    let absolute = run(
        root.path(),
        &["storage", "set-location", "data", "/Volumes/Work"],
    );
    let relative = run(
        root.path(),
        &["storage", "set-location", "logs", "work/logs"],
    );
    let builtin = run(
        root.path(),
        &["storage", "set-location", "cache", "--default"],
    );

    for run in [&absolute, &relative, &builtin] {
        assert_eq!(run.code, Some(0), "{run:?}");
    }
    assert!(
        absolute.stdout.contains("Existing data stays where it is."),
        "{}",
        absolute.stdout
    );
    let sent: Vec<_> = daemon
        .log()
        .into_iter()
        .filter(|logged| logged.method == "storage.set_location")
        .collect();
    assert_eq!(sent.len(), 3);
    let content = |n: usize| sent[n].raw["call"]["content"].clone();
    assert_eq!(content(0)["kind"], "data");
    assert_eq!(content(0)["target"]["type"], "path");
    assert_eq!(content(0)["target"]["content"], "/Volumes/Work");
    assert_eq!(content(0)["expectedRevision"], 3);
    let made = content(1)["target"]["content"]
        .as_str()
        .expect("a path")
        .to_owned();
    assert!(
        made.starts_with('/') && made.ends_with("work/logs"),
        "{made}"
    );
    assert_eq!(content(2)["target"]["type"], "builtIn");
    assert!(sent.iter().all(|logged| logged.idempotency_key.is_some()));
}

#[test]
fn a_refused_folder_exits_2_with_the_sentence_of_its_rule() {
    // covers: AC-19
    let root = TestRoot::new().expect("a test root is made");
    let _daemon = ScriptedDaemon::serve(&root, |call, _| match call {
        Call::SettingsGet => vec![Step::Reply(Reply::SettingsGet(Box::new(view(
            3, "manual", "stable", false,
        ))))],
        Call::StorageSetLocation(_) => vec![Step::Fail(refusal(
            "not_internal_volume",
            "Choose a folder on a disk inside this Mac. External disks aren’t supported yet.",
            "/Volumes/Backup",
        ))],
        _ => Vec::new(),
    });

    let run = run(
        root.path(),
        &[
            "storage",
            "set-location",
            "data",
            "/Volumes/Backup",
            "--json",
        ],
    );

    assert_eq!(run.code, Some(2), "{run:?}");
    let value = envelope(&run);
    assert_eq!(value["error"]["code"], "storage.location_invalid");
    assert_eq!(
        value["error"]["message"],
        "Choose a folder on a disk inside this Mac. External disks aren’t supported yet."
    );
    assert_eq!(value["error"]["details"][0]["value"], "not_internal_volume");
}

#[test]
fn set_location_needs_a_folder_or_default_and_not_both() {
    let root = TestRoot::new().expect("a test root is made");
    let daemon = ScriptedDaemon::serve(&root, |_, _| Vec::new());

    let neither = run(root.path(), &["storage", "set-location", "data"]);
    let both = run(
        root.path(),
        &[
            "storage",
            "set-location",
            "data",
            "/Volumes/Work",
            "--default",
        ],
    );

    assert_eq!(neither.code, Some(2), "{neither:?}");
    assert_eq!(both.code, Some(2), "{both:?}");
    assert!(daemon.log().is_empty(), "nothing was sent");
}
