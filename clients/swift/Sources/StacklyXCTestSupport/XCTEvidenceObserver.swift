import Foundation
import StacklyEvidenceCore
import Synchronization
import XCTest

/// Writes the final record of every test that called `evidence`, once XCTest
/// has finished it.
///
/// The helper installs one observer per process, on the first `evidence`
/// call. XCTest calls it off the main actor, which is why the state lives in
/// `XCTEvidenceTracker`.
final class XCTEvidenceObserver: NSObject, XCTestObservation, Sendable {
    /// The observer of this process, created and registered on first use.
    static let shared: XCTEvidenceObserver = {
        let observer = XCTEvidenceObserver()
        XCTestObservationCenter.shared.addTestObserver(observer)
        return observer
    }()

    /// The tests that called `evidence` and are still running.
    let tracker = XCTEvidenceTracker()

    func testCase(_ testCase: XCTestCase, didRecord issue: XCTIssue) {
        tracker.noteIssue(
            ObjectIdentifier(testCase), description: issue.compactDescription)
    }

    func testCaseDidFinish(_ testCase: XCTestCase) {
        guard let entry = tracker.finish(ObjectIdentifier(testCase)) else {
            return
        }
        let run = testCase.testRun
        let outcome = XCTEvidenceMapping.Outcome(
            failureCount: Int(run?.totalFailureCount ?? 0),
            unexpectedExceptionCount: Int(run?.unexpectedExceptionCount ?? 0),
            skipped: run?.hasBeenSkipped ?? false)
        guard let target = entry.target,
            let record = XCTEvidenceMapping.finalRecord(
                entry.notes, outcome: outcome, at: Date())
        else {
            return
        }
        do {
            try RecordFile.write(record, into: target.directory)
        } catch {
            // The test is over, so there is no issue to attach this to; the record stays
            // NOT_RUN, which the evidence report flags.
            FileHandle.standardError.write(
                Data(
                    ("evidence: cannot write the final record \(RecordFile.name(of: record.id)) "
                        + "to \(target.directory.path): \(error); it stays NOT_RUN\n").utf8))
        }
    }
}
