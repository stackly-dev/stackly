//! The Swift half of `evidence check`: the native test targets (spec 0002,
//! "Penanda test native").
//!
//! Swift source is read as text. Lines whose code starts with `//` are
//! skipped, so doc comments that show the rules are not flagged, and an
//! `@Test(...)` attribute is read up to its closing parenthesis even when it
//! spans several lines. A file fails when:
//!
//! - an `@Test` in a `*NativeTests` target folder has no `kind: .native`;
//! - `kind: .native` shows up outside such a folder;
//! - a file that uses `.evidence(` calls `Test.cancel(` itself;
//! - a UI test file (spec 0068: under `apps/macos/`, its name ending with
//!   `UITests`) that calls `evidence(covers:` also calls
//!   `XCTSkip(`, `XCTSkipIf(`, or `XCTSkipUnless(` itself, or writes `kind:`
//!   in the arguments of `evidence`.

/// The name of a UI test file ends with this.
const UI_TEST_SUFFIX: &str = "UITests";

/// Returns every rule violation in one Swift source file. `file` is the
/// repository relative path used in the messages, and `native_target` is
/// true when the file sits in a folder whose name ends with `NativeTests`.
pub fn scan(file: &str, source: &str, native_target: bool) -> Vec<String> {
    let code = without_comment_lines(source);
    let mut violations = Vec::new();
    let mut report = |at: usize, message: &str| {
        violations.push(format!("{file}:{}: {message}", line_of(&code, at)));
    };

    if native_target {
        for at in find_word(&code, "@Test") {
            match test_attribute(&code, at) {
                Some(attribute) if native_kind_at(attribute).is_some() => {}
                Some(_) => report(
                    at,
                    "an @Test in a NativeTests target needs .evidence(..., kind: .native, ...)",
                ),
                None => report(at, "@Test( has no closing parenthesis"),
            }
        }
    } else {
        let mut rest = 0;
        while let Some(offset) = native_kind_at(&code[rest..]) {
            report(
                rest + offset,
                "kind: .native belongs only in a test target whose name ends with NativeTests",
            );
            rest += offset + "kind:".len();
        }
    }

    if code.contains(".evidence(") {
        for at in find_word(&code, "Test.cancel(") {
            report(
                at,
                "a test with .evidence never calls Test.cancel itself; use Evidence.blocked",
            );
        }
    }
    if is_ui_test_file(file) {
        scan_ui_test(&code, &mut report);
    }
    violations
}

/// The UI test rules: the file records a `fixture` result only, and a missing
/// prerequisite ends the test with `XCTEvidence.blocked`.
fn scan_ui_test(code: &str, report: &mut impl FnMut(usize, &str)) {
    let calls = evidence_calls(code);
    if calls.is_empty() {
        return;
    }
    for name in ["XCTSkip", "XCTSkipIf", "XCTSkipUnless"] {
        for at in find_calls(code, name) {
            report(
                at,
                &format!(
                    "a UI test with evidence(covers:) never calls {name} itself; use XCTEvidence.blocked"
                ),
            );
        }
    }
    for (at, arguments) in calls {
        if let Some(offset) = find_word(arguments, "kind:")
            .into_iter()
            .find(|&kind| !arguments[..kind].ends_with(is_identifier_char))
        {
            report(
                at + offset,
                "a UI test never passes kind: to evidence(covers:); a UI test is always fixture",
            );
        }
    }
}

/// True for a file under `apps/macos/` whose name ends with `UITests`. The
/// folder does not count: `StacklyUITests` holds Swift Testing unit tests.
fn is_ui_test_file(file: &str) -> bool {
    let file = file.replace('\\', "/");
    let name = file.rsplit('/').next().unwrap_or_default();
    file.starts_with("apps/macos/")
        && name
            .strip_suffix(".swift")
            .unwrap_or(name)
            .ends_with(UI_TEST_SUFFIX)
}

/// The calls `evidence(covers:` of an `XCTestCase` in `code`, as the byte
/// offset of each argument list (the opening parenthesis included) and the
/// list itself. The `.evidence(` trait of Swift Testing is not one of them.
fn evidence_calls(code: &str) -> Vec<(usize, &str)> {
    find_calls(code, "evidence")
        .into_iter()
        .filter(|&at| !code[..at].ends_with('.') || code[..at - 1].ends_with("self"))
        .filter_map(|at| {
            let open = at + code[at..].find('(')?;
            let group = balanced_group(code, open)?;
            group
                .strip_prefix('(')?
                .trim_start()
                .starts_with("covers:")
                .then_some((open, group))
        })
        .collect()
}

/// Byte offsets of `name` used as a call: not part of a longer identifier,
/// and followed by an opening parenthesis (whitespace allowed in between).
fn find_calls(code: &str, name: &str) -> Vec<usize> {
    code.match_indices(name)
        .filter(|(at, _)| {
            !code[..*at].ends_with(is_identifier_char)
                && code[at + name.len()..].trim_start().starts_with('(')
        })
        .map(|(at, _)| at)
        .collect()
}

/// The source with every line whose code starts with `//` emptied, so byte
/// offsets still map to the same line numbers.
fn without_comment_lines(source: &str) -> String {
    source
        .split('\n')
        .map(|line| {
            if line.trim_start().starts_with("//") {
                ""
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn line_of(code: &str, at: usize) -> usize {
    code[..at].matches('\n').count() + 1
}

fn is_identifier_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Byte offsets of `word` where the next character does not continue an
/// identifier, so `@Test` does not match `@Tested`.
fn find_word(code: &str, word: &str) -> Vec<usize> {
    code.match_indices(word)
        .filter(|(at, _)| {
            let continues = word.ends_with(is_identifier_char)
                && code[at + word.len()..]
                    .chars()
                    .next()
                    .is_some_and(is_identifier_char);
            !continues
        })
        .map(|(at, _)| at)
        .collect()
}

/// The text of the `@Test` attribute at `at`: just `@Test`, or `@Test(...)`
/// through its closing parenthesis. Null when the parenthesis never closes.
fn test_attribute(code: &str, at: usize) -> Option<&str> {
    let after = at + "@Test".len();
    let rest = &code[after..];
    let open = rest.len() - rest.trim_start().len();
    if !rest[open..].starts_with('(') {
        return Some(&code[at..after]);
    }
    let group = balanced_group(code, after + open)?;
    Some(&code[at..after + open + group.len()])
}

/// The text from the opening parenthesis at `start` through its matching
/// closing one, skipping parentheses inside string literals. Null when the
/// parenthesis never closes.
fn balanced_group(code: &str, start: usize) -> Option<&str> {
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for (offset, c) in code[start..].char_indices() {
        if in_string {
            match (escaped, c) {
                (true, _) => escaped = false,
                (false, '\\') => escaped = true,
                (false, '"') => in_string = false,
                _ => {}
            }
            continue;
        }
        match c {
            '"' => in_string = true,
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&code[start..start + offset + 1]);
                }
            }
            _ => {}
        }
    }
    None
}

/// The byte offset of the first `kind:` followed by `.native` (whitespace
/// allowed in between), or null.
fn native_kind_at(text: &str) -> Option<usize> {
    find_word(text, "kind:").into_iter().find(|&at| {
        let value = text[at + "kind:".len()..].trim_start();
        value
            .strip_prefix(".native")
            .is_some_and(|after| !after.chars().next().is_some_and(is_identifier_char))
            && !text[..at].ends_with(is_identifier_char)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = "clients/swift/Tests/StacklyClientNativeTests/PostgresNativeTests.swift";

    #[test]
    fn native_tests_with_a_multi_line_native_trait_pass() {
        let source = r#"
import StacklyTestSupport
import Testing

struct PostgresNativeTests {
    @Test(
        .evidence(
            covers: ["AC-019"],
            kind: .native,
            expected: "the client sees PostgreSQL 18 (ready)"
        )
    )
    func clientSeesPostgres18Ready() async throws {
        guard let runtime = try await managedRuntime() else {
            try Evidence.blocked(reason: "no artifact", owner: "feature 3 owner")
        }
    }
}
"#;
        assert_eq!(scan(FILE, source, true), Vec::<String>::new());
    }

    #[test]
    fn an_at_test_in_a_native_target_without_native_kind_fails() {
        let source = r#"
struct PostgresNativeTests {
    @Test func plain() {}

    @Test(.evidence(covers: ["AC-019"], expected: "fixture by default"))
    func fixtureByDefault() {}

    @Test(.evidence(covers: ["AC-019"], expected: "a ) in a string", kind: .fixture))
    func fixture() {}
}
"#;
        let found = scan(FILE, source, true);
        assert_eq!(found.len(), 3, "{found:?}");
        assert!(found[0].starts_with(&format!("{FILE}:3: an @Test in a NativeTests target")));
        assert!(found[1].starts_with(&format!("{FILE}:5: ")));
        assert!(found[2].starts_with(&format!("{FILE}:8: ")));
    }

    #[test]
    fn native_kind_outside_a_native_target_fails() {
        let source = r#"
struct InstanceListTests {
    @Test(.evidence(covers: ["AC-047"], kind:   .native, expected: "x"))
    func one() {}

    let kind: EvidenceKind = .fixture
    let other = Thing(subkind: .native)
}
"#;
        let found = scan(
            "clients/swift/Tests/StacklyClientTests/X.swift",
            source,
            false,
        );
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].starts_with(
            "clients/swift/Tests/StacklyClientTests/X.swift:3: kind: .native belongs only"
        ));
    }

    #[test]
    fn comment_lines_are_skipped() {
        let source = r#"
/// Use it as `@Test(.evidence(covers: ["AC-019"], kind: .native, expected: "..."))`.
/// Never call `Test.cancel(_:)` directly; `.evidence(` handles it.
// kind: .native
public struct EvidenceTrait {}
public func blocked() throws -> Never {
    try Test.cancel("blocked")
}
"#;
        assert_eq!(
            scan(
                "clients/swift/Sources/StacklyTestSupport/Evidence.swift",
                source,
                false
            ),
            Vec::<String>::new()
        );
    }

    #[test]
    fn calling_test_cancel_next_to_the_evidence_trait_fails() {
        let source = r#"
struct InstanceListTests {
    @Test(.evidence(covers: ["AC-047"], expected: "x"))
    func one() throws {
        try Test.cancel("no daemon")
    }
}
"#;
        let found = scan(
            "clients/swift/Tests/StacklyClientTests/X.swift",
            source,
            false,
        );
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains(":5: a test with .evidence never calls Test.cancel"));
    }

    #[test]
    fn an_unclosed_attribute_is_reported() {
        let found = scan(FILE, "@Test(.evidence(kind: .native\nfunc x() {}\n", true);
        assert_eq!(found.len(), 1);
        assert!(found[0].contains(":1: @Test( has no closing parenthesis"));
    }

    const UI_FILE: &str = "apps/macos/StacklyAppUITests/ShellUITests.swift";

    #[test]
    fn a_ui_test_that_blocks_through_the_helper_passes() {
        let source = r#"
@MainActor
final class ShellUITests: XCTestCase {
    func testFooterReadsConnected() throws {
        try evidence(
            covers: ["AC-024"],
            expected: "the footer reads \"kind\" of connection (a ) in a string)"
        )
        guard automationReady else {
            try XCTEvidence.blocked(reason: "Automation Mode is off", owner: "user")
        }
    }
}
"#;
        assert_eq!(scan(UI_FILE, source, false), Vec::<String>::new());
    }

    #[test]
    fn a_ui_test_that_skips_directly_fails() {
        let source = r#"
final class ShellUITests: XCTestCase {
    func testA() throws {
        try evidence(covers: ["AC-024"], expected: "x")
        throw XCTSkip("no")
    }
    func testB() throws {
        try evidence(covers: ["AC-024"], expected: "x")
        try XCTSkipIf(true, "no")
        try XCTSkipUnless (false, "no")
    }
}
"#;
        let found = scan(UI_FILE, source, false);
        assert_eq!(found.len(), 3, "{found:?}");
        assert!(found.iter().any(|f| {
            f.contains(":5: a UI test with evidence(covers:) never calls XCTSkip itself")
        }));
        assert!(
            found
                .iter()
                .any(|f| f.contains(":9: ") && f.contains("XCTSkipIf"))
        );
        assert!(
            found
                .iter()
                .any(|f| f.contains(":10: ") && f.contains("XCTSkipUnless"))
        );
    }

    #[test]
    fn a_ui_test_that_passes_kind_to_evidence_fails() {
        let source = r#"
final class ShellUITests: XCTestCase {
    func testA() throws {
        try evidence(
            covers: ["AC-024"],
            kind: .fixture,
            expected: "x"
        )
    }
}
"#;
        let found = scan(UI_FILE, source, false);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains(":6: a UI test never passes kind: to evidence(covers:)"));
    }

    #[test]
    fn a_ui_test_file_without_an_evidence_call_may_skip() {
        let source = r#"
final class HelpersUITests {
    func wait() throws {
        throw XCTSkip("helper")
    }
}
"#;
        assert_eq!(scan(UI_FILE, source, false), Vec::<String>::new());
    }

    #[test]
    fn ui_test_comments_and_longer_names_are_not_calls() {
        let source = r#"
final class ShellUITests: XCTestCase {
    // throw XCTSkip("never")
    /// try evidence(covers: ["AC-024"], kind: .fixture, expected: "x")
    func testA() throws {
        try evidence(covers: ["AC-024"], expected: "x")
        try MyXCTSkip("a helper with another name")
        let kind: Kind = .fixture
    }
}
"#;
        assert_eq!(scan(UI_FILE, source, false), Vec::<String>::new());
    }

    #[test]
    fn the_swift_testing_trait_in_a_ui_named_file_is_not_a_call() {
        let source = r#"
struct ShellUITests {
    @Test(.evidence(covers: ["AC-024"], kind: .fixture, expected: "x"))
    func a() throws {
        try Foo.cancel()
        throw XCTSkip("no")
    }
}
"#;
        assert_eq!(scan(UI_FILE, source, false), Vec::<String>::new());
    }

    #[test]
    fn the_ui_test_rules_apply_only_under_apps_macos() {
        let source = r#"
final class ShellUITests: XCTestCase {
    func testA() throws {
        try evidence(covers: ["AC-024"], kind: .fixture, expected: "x")
        throw XCTSkip("no")
    }
}
"#;
        for file in [
            "clients/swift/Tests/StacklyXCTestSupportTests/ShellUITests.swift",
            "apps/macos/StacklyApp/ShellTests.swift",
            "apps/macos/Packages/StacklyUI/Tests/StacklyUITests/ShellTests.swift",
            "apps/macos/StacklyAppUITests/Support/Launch.swift",
        ] {
            assert_eq!(scan(file, source, false), Vec::<String>::new(), "{file}");
        }
        for file in [
            "apps/macos/StacklyAppUITests/ShellUITests.swift",
            "apps/macos/Other/GalleryUITests.swift",
            "apps/macos/StacklyAppUITests/MenuUITests.swift",
        ] {
            assert_eq!(scan(file, source, false).len(), 2, "{file}");
        }
    }
}
