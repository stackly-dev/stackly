//! The shared socket path fixture of spec 0013 (AC-1, AC-20): the Rust
//! layout and the Swift client derive the same runtime root and daemon
//! socket path from the same bytes.

use std::path::{Path, PathBuf};

use serde_json::Value;
use stackly_platform::{DirectoryLayout, PlatformError, RootSet, default_runtime_root};

/// Reads `protocols/fixtures/layout/socket-paths.json`.
fn fixture() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../protocols/fixtures/layout/socket-paths.json");
    let text = std::fs::read_to_string(&path).expect("the socket path fixture is readable");
    serde_json::from_str(&text).expect("the socket path fixture is JSON")
}

fn text<'a>(case: &'a Value, key: &str) -> &'a str {
    case[key]
        .as_str()
        .unwrap_or_else(|| panic!("case {} has a string {key}", case["name"]))
}

/// The root set for one case: the default runtime root under `tempDir`,
/// or the override's root set.
fn roots(case: &Value) -> Result<RootSet, PlatformError> {
    let temp = Path::new(text(case, "tempDir"));
    match case["override"].as_str() {
        None => RootSet::new(
            PathBuf::from("/fixture/data"),
            PathBuf::from("/fixture/logs"),
            PathBuf::from("/fixture/cache"),
            default_runtime_root(temp),
        ),
        Some(root) => RootSet::under_override(Path::new(root), temp),
    }
}

#[test]
fn every_fixture_case_gives_the_same_runtime_root_and_socket_path() {
    let fixture = fixture();
    let cases = fixture["cases"].as_array().expect("cases is an array");
    assert!(cases.len() >= 8, "the fixture keeps its minimum cases");
    for case in cases {
        let name = text(case, "name");
        match case.get("error").and_then(Value::as_str) {
            Some("runtime_root_too_long") => {
                let error = roots(case).expect_err(name);
                assert!(
                    matches!(error, PlatformError::RuntimeRootTooLong { .. }),
                    "{name}: {error}"
                );
            }
            Some(other) => panic!("{name}: unknown error {other}"),
            None => {
                let roots = roots(case).unwrap_or_else(|error| panic!("{name}: {error}"));
                assert_eq!(
                    roots.runtime().as_os_str().as_encoded_bytes(),
                    text(case, "runtimeRoot").as_bytes(),
                    "{name}"
                );
                let socket = DirectoryLayout::new(roots).daemon_socket();
                assert_eq!(
                    socket.as_os_str().as_encoded_bytes(),
                    text(case, "socketPath").as_bytes(),
                    "{name}"
                );
            }
        }
    }
}
