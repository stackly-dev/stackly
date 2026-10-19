//! Golden files under `apps/cli/tests/golden/`. Set
//! `STACKLY_UPDATE_GOLDEN=1` to write them again. Ages are normalized here;
//! times come from fixed DTOs with `TZ=UTC`, and the time format itself is
//! unit tested with a fixed `now` and zone.

use std::path::PathBuf;

use regex::Regex;
use stackly_protocol::{CONTRACT_DIGEST, ProtocolVersion};

/// The folder of golden files.
pub fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

/// `text` with every age (`3m ago`, `now`) in brackets written as `<age>`,
/// and this build's contract digest and protocol version as placeholders,
/// so a protocol minor bump does not move the goldens.
pub fn normalize(text: &str) -> String {
    let ages = Regex::new(r"\((?:\d+[smhd] ago|now)\)").expect("the age pattern is valid");
    let protocol = ProtocolVersion::CURRENT;
    ages.replace_all(text, "(<age>)")
        .replace(CONTRACT_DIGEST, "<contract-digest>")
        .replace(
            &format!(
                "{{\"major\":{},\"minor\":{}}}",
                protocol.major, protocol.minor
            ),
            "{\"major\":<major>,\"minor\":<minor>}",
        )
        .replace(
            &format!("(protocol {}.{})", protocol.major, protocol.minor),
            "(protocol <major>.<minor>)",
        )
}

/// Compares `actual`, normalized, with the golden file `name`.
pub fn check(name: &str, actual: &str) {
    let actual = normalize(actual);
    let path = dir().join(name);
    if std::env::var_os("STACKLY_UPDATE_GOLDEN").is_some_and(|value| value == "1") {
        std::fs::create_dir_all(dir()).expect("the golden folder can be created");
        std::fs::write(&path, &actual).expect("the golden file can be written");
        return;
    }
    let expected = std::fs::read_to_string(&path).unwrap_or_else(|_| {
        panic!(
            "golden file {} is missing; run with STACKLY_UPDATE_GOLDEN=1",
            path.display()
        )
    });
    assert_eq!(
        actual,
        expected,
        "{} differs; run with STACKLY_UPDATE_GOLDEN=1 to accept",
        path.display()
    );
}
