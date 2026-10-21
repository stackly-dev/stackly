import Foundation
import StacklyEvidenceCore
import XCTest

@testable import StacklyXCTestSupport

/// What the observer's state keeps while a test runs.
final class XCTEvidenceTrackerTests: XCTestCase {
    private let key = ObjectIdentifier(NSObject())

    private func record() -> Record {
        let now = Date(timeIntervalSince1970: 1_792_300_000)
        return Record(
            schema: Record.schemaName, id: .version7(at: now), covers: ["AC-024"],
            kind: .fixture, method: .automated,
            test: TestIdentity(
                language: .swift, package: nil, target: "T", name: "C/m()", file: nil, line: nil),
            status: .notRun, expected: "x", actual: nil, reason: nil, owner: nil, subjects: [],
            steps: nil, reviewer: nil, startedAt: now, finishedAt: nil,
            evidencePath: Record.outputLog)
    }

    func testOnlyTheFirstIssueIsKept() throws {
        let tracker = XCTEvidenceTracker()
        XCTAssertTrue(tracker.begin(key, record: record(), target: nil))
        tracker.noteIssue(key, description: "first")
        tracker.noteIssue(key, description: "second")
        let entry = try XCTUnwrap(tracker.finish(key))
        XCTAssertEqual(entry.notes.firstIssue, "first")
    }

    func testASecondBeginForTheSameTestIsRefused() {
        let tracker = XCTEvidenceTracker()
        XCTAssertTrue(tracker.begin(key, record: record(), target: nil))
        XCTAssertFalse(tracker.begin(key, record: record(), target: nil))
    }

    func testActualAndBlockedAreRefusedWhenNoTestIsTracked() {
        let tracker = XCTEvidenceTracker()
        XCTAssertFalse(tracker.noteActual("x"))
        XCTAssertFalse(tracker.noteBlocked(reason: "r", owner: "o"))
    }

    func testActualAndBlockedGoToTheTrackedTestAndLaterCallsReplaceEarlierOnes() throws {
        let tracker = XCTEvidenceTracker()
        XCTAssertTrue(tracker.begin(key, record: record(), target: nil))
        XCTAssertTrue(tracker.noteActual("one"))
        XCTAssertTrue(tracker.noteActual("two"))
        XCTAssertTrue(tracker.noteBlocked(reason: "r", owner: "o"))
        let entry = try XCTUnwrap(tracker.finish(key))
        XCTAssertEqual(entry.notes.actual, "two")
        XCTAssertEqual(entry.notes.blocked?.reason, "r")
        XCTAssertEqual(entry.notes.blocked?.owner, "o")
    }

    func testFinishForgetsTheTestAndStopsAcceptingNotes() {
        let tracker = XCTEvidenceTracker()
        XCTAssertTrue(tracker.begin(key, record: record(), target: nil))
        XCTAssertNotNil(tracker.finish(key))
        XCTAssertNil(tracker.finish(key))
        XCTAssertFalse(tracker.isTracking(key))
        XCTAssertFalse(tracker.noteActual("late"))
    }
}
