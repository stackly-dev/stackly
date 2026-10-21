import Foundation
import StacklyEvidenceCore
import XCTest

/// Notes made by a test that called `evidence(covers:expected:)` (spec 0068).
///
/// The XCTest padanan of `Evidence` in `StacklyTestSupport`: the record format
/// and the rules are the same.
public enum XCTEvidence {
    /// States what a passing test observed. Without it a `PASS` record says
    /// `all expectations passed`.
    ///
    /// An empty value, or a call outside a test that called `evidence`,
    /// records a failure. A later call replaces an earlier value.
    ///
    /// - Parameters:
    ///   - value: The observed value, for example `ready on port 54001`.
    ///   - file: Where the failure points when the call is wrong.
    ///   - line: The line of the failure.
    public static func actual(_ value: String, file: StaticString = #filePath, line: UInt = #line) {
        guard !Rules.isBlank(value) else {
            XCTFail("XCTEvidence.actual needs the observed value", file: file, line: line)
            return
        }
        guard XCTEvidenceObserver.shared.tracker.noteActual(value) else {
            XCTFail(outsideTest("XCTEvidence.actual"), file: file, line: line)
            return
        }
    }

    /// Marks the record `BLOCKED` because a prerequisite (binary, hardware,
    /// signing) is missing, then skips the test with `XCTSkip`, so it is
    /// reported skipped rather than passed. It never returns, so it can end
    /// the `else` of a `guard`.
    ///
    /// An empty reason or owner records a failure, so the record is `FAIL`.
    ///
    /// - Parameters:
    ///   - reason: What is missing.
    ///   - owner: Who can unblock it.
    ///   - file: Where the failure points when the call is wrong.
    ///   - line: The line of the failure.
    /// - Throws: `XCTSkip`, or, outside a test that called `evidence`, an
    ///   error that fails the test.
    public static func blocked(
        reason: String, owner: String, file: StaticString = #filePath, line: UInt = #line
    ) throws -> Never {
        if Rules.isBlank(reason) || Rules.isBlank(owner) {
            XCTFail(
                "XCTEvidence.blocked needs a reason and the owner who can unblock it", file: file,
                line: line)
        } else if !XCTEvidenceObserver.shared.tracker.noteBlocked(reason: reason, owner: owner) {
            let message = outsideTest("XCTEvidence.blocked")
            XCTFail(message, file: file, line: line)
            throw XCTEvidenceError(description: message)
        }
        throw XCTSkip(reason, file: file, line: line)
    }

    private static func outsideTest(_ function: String) -> String {
        "\(function) was called outside a test that called evidence, so nothing records it"
    }
}
