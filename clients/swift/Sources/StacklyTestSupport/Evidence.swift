public import Testing

/// Evidence records for Swift tests that prove PRD IDs (spec 0002).
///
/// A test that proves a PRD ID (AC, REQ, NFR, VAL, or BR) carries the
/// evidence trait. The trait writes one `stackly.evidence/v1` record per test
/// function in a run of `cargo xtask evidence run`: `NOT_RUN` when the test
/// starts, then `PASS`, `FAIL`, or `BLOCKED`. Without a run it writes nothing
/// but still checks its arguments.
///
/// ```swift
/// @Test(.evidence(covers: ["AC-047"], expected: "the client refuses another protocol major"))
/// func clientRefusesAnotherProtocolMajor() async throws {
///     // #expect and #require as usual
/// }
/// ```
///
/// A native test lives in a test target whose name ends with `NativeTests`,
/// declares `kind: .native`, names the runtime it found with
/// `Evidence.subject`, and states what it observed with `Evidence.actual`. A
/// missing prerequisite ends the test with `Evidence.blocked`, never with a
/// direct cancel.
public enum Evidence {
    /// The scope of the test that is running, set by the evidence trait.
    @TaskLocal static var current: Scope?

    /// Adds a provider or runtime the test ran against. Call it as soon as the
    /// runtime is found, with the version read from the running binary, never
    /// a literal.
    ///
    /// An inexact version (such as `18`), a digest that is not 64 lowercase
    /// hex characters, a native test with source `fixture`, or a call outside
    /// a test with the evidence trait records an issue, so the record is
    /// `FAIL`.
    ///
    /// - Parameters:
    ///   - provider: The provider, for example `postgresql`.
    ///   - version: The exact version the running binary reported.
    ///   - digest: The SHA-256 of the artifact as 64 lowercase hex
    ///     characters, or nil.
    ///   - source: Where the runtime came from.
    ///   - sourceLocation: Where the issue points when the subject is wrong.
    public static func subject(
        provider: String, version: String, digest: String?, source: Source,
        sourceLocation: SourceLocation = #_sourceLocation
    ) {
        guard let scope = scope(for: "Evidence.subject", at: sourceLocation) else {
            return
        }
        let problem: String? =
            if Rules.isBlank(provider) {
                "a subject needs a provider"
            } else if !Rules.isExactVersion(version) {
                "subject \(provider): version `\(version)` is not exact"
            } else if let digest, !Rules.isSHA256Hex(digest) {
                "subject \(provider): the digest must be 64 lowercase hex characters"
            } else if scope.kind == .native && source == .fixture {
                "subject \(provider): a native test never uses source fixture"
            } else {
                nil
            }
        if let problem {
            Issue.record(Comment(rawValue: problem), sourceLocation: sourceLocation)
            return
        }
        scope.addSubject(
            Subject(provider: provider, version: version, digest: digest, source: source))
    }

    /// States what a passing test observed. Without it a `PASS` record says
    /// `all expectations passed`, which a native `PASS` may not use.
    ///
    /// An empty value, or a call outside a test with the evidence trait,
    /// records an issue. A later call replaces an earlier value.
    ///
    /// - Parameters:
    ///   - value: The observed value, for example `ready on port 54001`.
    ///   - sourceLocation: Where the issue points when the call is wrong.
    public static func actual(_ value: String, sourceLocation: SourceLocation = #_sourceLocation) {
        guard let scope = scope(for: "Evidence.actual", at: sourceLocation) else {
            return
        }
        guard !Rules.isBlank(value) else {
            Issue.record("Evidence.actual needs the observed value", sourceLocation: sourceLocation)
            return
        }
        scope.noteActual(value)
    }

    /// Marks the record `BLOCKED` because a prerequisite (binary, hardware,
    /// signing) is missing, then cancels the test, so it is reported
    /// cancelled rather than passed. It never returns, so it can end the
    /// `else` of a `guard`.
    ///
    /// An empty reason or owner records an issue, so the record is `FAIL`.
    ///
    /// - Parameters:
    ///   - reason: What is missing.
    ///   - owner: Who can unblock it.
    ///   - sourceLocation: Where the issue points when the call is wrong.
    /// - Throws: The cancellation of the test, or, outside a test with the
    ///   evidence trait, an error that fails the test.
    public static func blocked(
        reason: String, owner: String, sourceLocation: SourceLocation = #_sourceLocation
    ) throws -> Never {
        guard let scope = current else {
            throw EvidenceSetupError(description: outsideScope("Evidence.blocked"))
        }
        if Rules.isBlank(reason) || Rules.isBlank(owner) {
            Issue.record(
                "Evidence.blocked needs a reason and the owner who can unblock it",
                sourceLocation: sourceLocation)
        } else {
            scope.noteBlocked(reason: reason, owner: owner)
        }
        try scope.cancel(reason)
    }

    /// The running scope, or nil after recording an issue for `function`.
    private static func scope(for function: String, at sourceLocation: SourceLocation) -> Scope? {
        guard let scope = current else {
            Issue.record(Comment(rawValue: outsideScope(function)), sourceLocation: sourceLocation)
            return nil
        }
        return scope
    }

    private static func outsideScope(_ function: String) -> String {
        "\(function) was called outside a test with the evidence trait, so nothing records it"
    }
}
