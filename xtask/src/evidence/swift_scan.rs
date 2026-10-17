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
//! - a file that uses `.evidence(` calls `Test.cancel(` itself.

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
    violations
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
    let start = after + open;
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
                    return Some(&code[at..start + offset + 1]);
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
}
