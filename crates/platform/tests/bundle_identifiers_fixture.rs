//! The shared bundle identifier fixture of spec 0015 (AC-1): the Rust
//! constants in `stackly-platform::bundle` and the Swift
//! `AgentIdentifiers` hold the same temporary identifiers (OQ-001).

use std::path::Path;

use serde_json::Value;
use stackly_platform::bundle;

/// Reads `protocols/fixtures/bundle/identifiers.json`.
fn fixture() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../protocols/fixtures/bundle/identifiers.json");
    let text = std::fs::read_to_string(&path).expect("the bundle identifier fixture is readable");
    serde_json::from_str(&text).expect("the bundle identifier fixture is JSON")
}

#[test]
fn every_rust_constant_matches_the_shared_fixture() {
    let fixture = fixture();
    let expected = [
        ("appBundleId", bundle::APP_BUNDLE_ID),
        ("daemonLabel", bundle::DAEMON_LABEL),
        ("daemonPlistFile", bundle::DAEMON_PLIST_FILE),
        ("daemonSigningId", bundle::DAEMON_SIGNING_ID),
        ("cliSigningId", bundle::CLI_SIGNING_ID),
        ("testAgentLabelPrefix", bundle::TEST_AGENT_LABEL_PREFIX),
        ("appExecutable", bundle::APP_EXECUTABLE),
        ("daemonExecutable", bundle::DAEMON_EXECUTABLE),
        ("cliExecutable", bundle::CLI_EXECUTABLE),
        ("macosDir", bundle::MACOS_DIR),
        ("launchAgentsDir", bundle::LAUNCH_AGENTS_DIR),
        ("resourcesDir", bundle::RESOURCES_DIR),
        ("daemonBundleProgram", bundle::DAEMON_BUNDLE_PROGRAM),
    ];
    for (key, value) in expected {
        assert_eq!(fixture[key].as_str(), Some(value), "{key}");
    }
    let keys = fixture.as_object().expect("the fixture is an object").len();
    assert_eq!(
        keys,
        expected.len(),
        "the fixture has a key without a constant"
    );
}

#[test]
fn the_plist_file_is_the_label_with_the_plist_extension() {
    assert_eq!(
        bundle::DAEMON_PLIST_FILE,
        format!("{}.plist", bundle::DAEMON_LABEL)
    );
}
