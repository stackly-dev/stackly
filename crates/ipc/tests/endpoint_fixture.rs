//! The shared endpoint fixture of spec 0013 (AC-2, AC-20): the order of the
//! trust checks and the `details.kind` of `ipc.io_error` are the same in Rust
//! and in the Swift client.

use std::io;
use std::path::Path;

use serde_json::Value;
use stackly_ipc::{PathFacts, PathKind, evaluate_path, io_kind_name};

fn fixture() -> Value {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../protocols/fixtures/endpoint/checks.json");
    let text = std::fs::read_to_string(&path).expect("the endpoint fixture is readable");
    serde_json::from_str(&text).expect("the endpoint fixture is JSON")
}

fn facts(entry: &Value) -> PathFacts {
    let kind = match entry["type"].as_str().expect("type is a string") {
        "socket" => PathKind::Socket,
        "dir" => PathKind::Directory,
        "symlink" => PathKind::Symlink,
        "file" => PathKind::Other,
        other => panic!("unknown path type {other}"),
    };
    let uid = entry["uid"].as_u64().expect("uid is a number");
    let mode = entry["mode"].as_str().expect("mode is an octal string");
    PathFacts::new(
        kind,
        u32::try_from(uid).expect("uid fits in a u32"),
        u32::from_str_radix(mode, 8).expect("mode is octal"),
    )
}

#[test]
fn every_check_fails_with_the_fixture_reason_in_the_fixture_order() {
    let fixture = fixture();
    let checks = fixture["checks"].as_array().expect("checks is an array");
    assert!(checks.len() >= 15, "the fixture keeps its cases");
    for check in checks {
        let name = check["name"].as_str().expect("name is a string");
        let uid = u32::try_from(check["uid"].as_u64().expect("uid is a number"))
            .expect("uid fits in a u32");
        let reason = evaluate_path(facts(&check["path"]), facts(&check["parent"]), uid);
        assert_eq!(
            reason.map(|reason| reason.as_str()),
            check["reason"].as_str(),
            "{name}"
        );
    }
}

#[test]
fn every_fixture_errno_maps_to_its_kind_name() {
    let fixture = fixture();
    let table = fixture["errno"].as_array().expect("errno is an array");
    let names: Vec<_> = table
        .iter()
        .filter_map(|entry| entry["errno"].as_str())
        .collect();
    for required in [
        "EACCES",
        "EPERM",
        "ENOENT",
        "ENAMETOOLONG",
        "ENOTDIR",
        "EMFILE",
        "ENFILE",
        "ELOOP",
    ] {
        assert!(names.contains(&required), "the fixture lists {required}");
    }
    let mut fallback = 0;
    for entry in table {
        let name = entry["errno"].as_str().expect("errno is a string");
        // The number is the Darwin value, part of the macOS ABI.
        let number = i32::try_from(entry["number"].as_i64().expect("number is a number"))
            .expect("an errno fits in an i32");
        let kind = io_kind_name(io::Error::from_raw_os_error(number).kind());
        let expected = entry["kind"].as_str().expect("kind is a string");
        assert_eq!(kind, expected, "{name}");
        if expected == "uncategorized" {
            fallback += 1;
        }
    }
    assert!(
        fallback > 0,
        "the fixture lists an errno with the fallback kind"
    );
}
