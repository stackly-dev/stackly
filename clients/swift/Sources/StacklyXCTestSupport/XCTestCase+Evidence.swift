import Foundation
import StacklyEvidenceCore
public import XCTest

extension XCTestCase {
    /// Marks the running test as evidence for `covers`, the XCTest padanan of
    /// the `.evidence` trait (spec 0068).
    ///
    /// Call it on the first line of the test with `try`. It writes the
    /// `NOT_RUN` record to the run folder of `cargo xtask evidence run`, and
    /// once the test ends it writes the final one: `BLOCKED` after
    /// `XCTEvidence.blocked`, else `FAIL` for a failure or an unexpected
    /// exception, else `NOT_RUN` for a skip, else `PASS`. Without a run it
    /// writes nothing but still checks its arguments. The kind is always
    /// `fixture`; in a native run the test records a failure.
    ///
    /// ```swift
    /// func testFooterReadsConnected() throws {
    ///     try evidence(covers: ["AC-024"], expected: "the footer reads connected")
    ///     // XCTAssert... as usual
    /// }
    /// ```
    ///
    /// - Parameters:
    ///   - covers: The PRD IDs the test proves, for example `["AC-024"]`.
    ///   - expected: The expected behavior in one sentence.
    ///   - file: Where the record says the test lives.
    ///   - line: The line of the record.
    /// - Throws: `XCTEvidenceError` after recording a failure, when the
    ///   arguments or the run environment are wrong or the test already
    ///   called `evidence`, so the test stops.
    public func evidence(
        covers: [String], expected: String, file: StaticString = #filePath,
        line: UInt = #line
    ) throws {
        try evidence(
            covers: covers, expected: expected, environment: ProcessInfo.processInfo.environment,
            file: file, line: line)
    }

    /// The same as the public `evidence`, with the process environment passed in so tests of this
    /// package never set environment variables.
    func evidence(
        covers: [String], expected: String, environment: [String: String],
        file: StaticString = #filePath, line: UInt = #line
    ) throws {
        let plan = RunPlan.make(
            covers: covers, kind: .fixture, expected: expected, environment: environment)
        let target: RunTarget?
        let mismatch: String?
        switch plan {
        case .reject(let message), .cancelNative(let message):
            XCTFail(message, file: file, line: line)
            throw XCTEvidenceError(description: message)
        case .run(let runTarget, let runMismatch):
            target = runTarget
            mismatch = runMismatch
        }

        let started = startedRecord(covers: covers, expected: expected, file: file, line: line)
        if let target {
            do {
                try RecordFile.write(started, into: target.directory)
            } catch {
                let message =
                    "evidence: cannot write the NOT_RUN record to \(target.directory.path): \(error)"
                XCTFail(message, file: file, line: line)
                throw XCTEvidenceError(description: message)
            }
        }
        let tracker = XCTEvidenceObserver.shared.tracker
        guard tracker.begin(ObjectIdentifier(self), record: started, target: target) else {
            let message = "evidence: only one evidence call per test"
            XCTFail(message, file: file, line: line)
            throw XCTEvidenceError(description: message)
        }
        // Fail only after the test is tracked, so the observer keeps this issue as the actual.
        if let mismatch {
            XCTFail(mismatch, file: file, line: line)
        }
    }

    /// The `NOT_RUN` record of the running test.
    private func startedRecord(
        covers: [String], expected: String, file: StaticString, line: UInt, at now: Date = Date()
    ) -> Record {
        Record(
            schema: Record.schemaName, id: .version7(at: now), covers: covers, kind: .fixture,
            method: .automated,
            test: TestIdentity(
                language: .swift, package: nil,
                target: XCTEvidenceMapping.moduleName(
                    reflecting: String(reflecting: type(of: self))),
                name: XCTEvidenceMapping.testName(xctestName: name),
                file: String(describing: file), line: Int(line)),
            status: .notRun, expected: expected, actual: nil, reason: nil, owner: nil,
            subjects: [], steps: nil, reviewer: nil, startedAt: now, finishedAt: nil,
            evidencePath: Record.outputLog)
    }
}

/// A broken evidence argument or environment, thrown after a failure was
/// recorded so the test stops.
public struct XCTEvidenceError: Error, CustomStringConvertible {
    /// What is wrong, as a lowercase sentence without a trailing period.
    public let description: String
}
