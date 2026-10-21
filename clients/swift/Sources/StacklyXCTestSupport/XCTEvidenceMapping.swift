import Foundation
import StacklyEvidenceCore

/// The pure parts of the XCTest evidence helper: how a test is named and how
/// its result becomes the status of a record (spec 0068, AC-10).
enum XCTEvidenceMapping {
    /// What XCTest reported about a finished test.
    struct Outcome: Equatable, Sendable {
        /// `testRun.totalFailureCount`.
        var failureCount: Int
        /// `testRun.unexpectedExceptionCount`.
        var unexpectedExceptionCount: Int
        /// `testRun.hasBeenSkipped`.
        var skipped: Bool

        /// True when the run holds a failure or an unexpected exception.
        var failed: Bool {
            failureCount > 0 || unexpectedExceptionCount > 0
        }
    }

    /// What the helper noted while a test ran.
    struct Notes {
        /// The `NOT_RUN` record the test started with.
        var record: Record
        /// The reason and owner from `XCTEvidence.blocked`, if it was called.
        var blocked: (reason: String, owner: String)?
        /// The observed value from `XCTEvidence.actual`, if any.
        var actual: String?
        /// The description of the first issue XCTest recorded, if any.
        var firstIssue: String?
    }

    /// The `actual` of a `FAIL` whose issue description was never observed.
    static let unknownFailure = "the test failed"

    /// The final record of a test, or nil when the record stays `NOT_RUN`.
    ///
    /// `BLOCKED` wins over a failure, then a failure or an unexpected
    /// exception means `FAIL` with the first issue as `actual`, then a skip
    /// without `blocked` stays `NOT_RUN`, and only then is it `PASS`.
    static func finalRecord(_ notes: Notes, outcome: Outcome, at now: Date) -> Record? {
        let failure = outcome.failed ? (notes.firstIssue ?? unknownFailure) : nil
        return notes.record.finished(
            blocked: notes.blocked, firstFailure: failure, unobserved: outcome.skipped,
            actual: notes.actual, at: now)
    }

    /// The test name of a record, `<Class>/<method>()`, from the XCTest name
    /// `-[Module.Class testSomething]`.
    ///
    /// The `AndReturnError:` suffix Swift adds to a throwing method and the
    /// `WithCompletionHandler:` suffix of an async one are dropped, and so is
    /// the module prefix of the class. A name that does not have the
    /// `-[Class selector]` shape is used as it is.
    static func testName(xctestName: String) -> String {
        guard xctestName.hasPrefix("-["), xctestName.hasSuffix("]") else {
            return xctestName
        }
        let inner = xctestName.dropFirst(2).dropLast()
        let parts = inner.split(separator: " ", maxSplits: 1, omittingEmptySubsequences: true)
        guard parts.count == 2 else {
            return xctestName
        }
        return "\(withoutModule(String(parts[0])))/\(method(of: String(parts[1])))()"
    }

    /// The module of a test class, from the reflected name `Module.Class`.
    static func moduleName(reflecting className: String) -> String {
        className.split(separator: ".", maxSplits: 1).first.map(String.init) ?? className
    }

    private static func withoutModule(_ qualified: String) -> String {
        let parts = qualified.split(separator: ".", maxSplits: 1)
        return parts.count == 2 ? String(parts[1]) : qualified
    }

    private static func method(of selector: String) -> String {
        var name = selector
        if name.hasSuffix(":") {
            name.removeLast()
        }
        for suffix in ["AndReturnError", "WithCompletionHandler"] where name.hasSuffix(suffix) {
            name.removeLast(suffix.count)
            break
        }
        return name
    }
}
