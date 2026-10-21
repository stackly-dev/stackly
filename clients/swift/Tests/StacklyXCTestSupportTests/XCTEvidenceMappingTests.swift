import Foundation
import StacklyEvidenceCore
import XCTest

@testable import StacklyXCTestSupport

/// The pure status and name rules of the XCTest evidence helper (spec 0068,
/// AC-10), one row of the mapping per test.
final class XCTEvidenceMappingTests: XCTestCase {
    private let started = Date(timeIntervalSince1970: 1_792_300_000)
    private let finished = Date(timeIntervalSince1970: 1_792_300_002)

    private func notes(
        blocked: (reason: String, owner: String)? = nil, actual: String? = nil,
        firstIssue: String? = nil
    ) -> XCTEvidenceMapping.Notes {
        let record = Record(
            schema: Record.schemaName, id: .version7(at: started), covers: ["AC-024"],
            kind: .fixture, method: .automated,
            test: TestIdentity(
                language: .swift, package: nil, target: "StacklyAppUITests",
                name: "ShellUITests/footerReadsConnected()", file: "/repo/ShellUITests.swift",
                line: 12),
            status: .notRun, expected: "the footer reads connected", actual: nil, reason: nil,
            owner: nil, subjects: [], steps: nil, reviewer: nil, startedAt: started,
            finishedAt: nil, evidencePath: Record.outputLog)
        return XCTEvidenceMapping.Notes(
            record: record, blocked: blocked, actual: actual, firstIssue: firstIssue)
    }

    private func outcome(
        failures: Int = 0, exceptions: Int = 0, skipped: Bool = false
    ) -> XCTEvidenceMapping.Outcome {
        XCTEvidenceMapping.Outcome(
            failureCount: failures, unexpectedExceptionCount: exceptions, skipped: skipped)
    }

    func testAPassingTestIsPassWithTheDefaultActual() throws {
        let record = try XCTUnwrap(
            XCTEvidenceMapping.finalRecord(notes(), outcome: outcome(), at: finished))
        XCTAssertEqual(record.status, .pass)
        XCTAssertEqual(record.actual, "all expectations passed")
        XCTAssertEqual(record.finishedAt, finished)
        XCTAssertNil(record.reason)
        XCTAssertNil(record.owner)
    }

    func testAPassingTestKeepsTheObservedValueItNoted() throws {
        let record = try XCTUnwrap(
            XCTEvidenceMapping.finalRecord(
                notes(actual: "footer read Status: Connected"), outcome: outcome(), at: finished))
        XCTAssertEqual(record.status, .pass)
        XCTAssertEqual(record.actual, "footer read Status: Connected")
    }

    func testAFailureIsFailWithTheFirstIssueAsActual() throws {
        let record = try XCTUnwrap(
            XCTEvidenceMapping.finalRecord(
                notes(actual: "ignored", firstIssue: "XCTAssertEqual failed: (a) is not (b)"),
                outcome: outcome(failures: 2), at: finished))
        XCTAssertEqual(record.status, .fail)
        XCTAssertEqual(record.actual, "XCTAssertEqual failed: (a) is not (b)")
        XCTAssertEqual(record.finishedAt, finished)
    }

    func testAnUnexpectedExceptionIsFail() throws {
        let record = try XCTUnwrap(
            XCTEvidenceMapping.finalRecord(
                notes(firstIssue: "caught error"), outcome: outcome(exceptions: 1), at: finished))
        XCTAssertEqual(record.status, .fail)
        XCTAssertEqual(record.actual, "caught error")
    }

    func testAFailureWithoutAnObservedIssueStillSaysItFailed() throws {
        let record = try XCTUnwrap(
            XCTEvidenceMapping.finalRecord(
                notes(), outcome: outcome(failures: 1), at: finished))
        XCTAssertEqual(record.status, .fail)
        XCTAssertEqual(record.actual, "the test failed")
    }

    func testAnIssueThatIsNotAFailureDoesNotFailTheRecord() throws {
        let record = try XCTUnwrap(
            XCTEvidenceMapping.finalRecord(
                notes(firstIssue: "expected failure"), outcome: outcome(), at: finished))
        XCTAssertEqual(record.status, .pass)
    }

    func testBlockedIsBlockedWithReasonAndOwnerAndNoActual() throws {
        let record = try XCTUnwrap(
            XCTEvidenceMapping.finalRecord(
                notes(blocked: ("Automation Mode is off", "@ojiepermana"), actual: "x"),
                outcome: outcome(skipped: true), at: finished))
        XCTAssertEqual(record.status, .blocked)
        XCTAssertEqual(record.reason, "Automation Mode is off")
        XCTAssertEqual(record.owner, "@ojiepermana")
        XCTAssertNil(record.actual)
        XCTAssertEqual(record.finishedAt, finished)
    }

    func testBlockedWinsOverAFailure() throws {
        let record = try XCTUnwrap(
            XCTEvidenceMapping.finalRecord(
                notes(blocked: ("no GUI login session", "user"), firstIssue: "boom"),
                outcome: outcome(failures: 1, skipped: true), at: finished))
        XCTAssertEqual(record.status, .blocked)
        XCTAssertNil(record.actual)
    }

    func testASkipWithoutBlockedLeavesTheRecordNotRun() {
        XCTAssertNil(
            XCTEvidenceMapping.finalRecord(
                notes(), outcome: outcome(skipped: true), at: finished))
    }

    func testAFailureWinsOverASkipWithoutBlocked() throws {
        let record = try XCTUnwrap(
            XCTEvidenceMapping.finalRecord(
                notes(firstIssue: "boom"), outcome: outcome(failures: 1, skipped: true),
                at: finished))
        XCTAssertEqual(record.status, .fail)
    }

    func testTheFinalRecordKeepsEveryFieldThatWasNotFinished() throws {
        let base = notes()
        let record = try XCTUnwrap(
            XCTEvidenceMapping.finalRecord(base, outcome: outcome(), at: finished))
        XCTAssertEqual(record.id, base.record.id)
        XCTAssertEqual(record.kind, .fixture)
        XCTAssertEqual(record.covers, ["AC-024"])
        XCTAssertEqual(record.test, base.record.test)
        XCTAssertEqual(record.startedAt, started)
        XCTAssertEqual(record.evidencePath, "output.log")
    }

    func testAThrowingMethodLosesTheAndReturnErrorSuffix() {
        XCTAssertEqual(
            XCTEvidenceMapping.testName(
                xctestName: "-[StacklyAppUITests.ShellUITests testFooterAndReturnError:]"),
            "ShellUITests/testFooter()")
    }

    func testAnAsyncMethodLosesTheWithCompletionHandlerSuffix() {
        XCTAssertEqual(
            XCTEvidenceMapping.testName(
                xctestName: "-[StacklyAppUITests.ShellUITests testFooterWithCompletionHandler:]"),
            "ShellUITests/testFooter()")
    }

    func testAPlainMethodKeepsItsName() {
        XCTAssertEqual(
            XCTEvidenceMapping.testName(xctestName: "-[Mod.GalleryUITests testStatus]"),
            "GalleryUITests/testStatus()")
    }

    func testAClassWithoutAModulePrefixIsKept() {
        XCTAssertEqual(
            XCTEvidenceMapping.testName(xctestName: "-[GalleryUITests testStatus]"),
            "GalleryUITests/testStatus()")
    }

    func testANestedClassKeepsItsPathBelowTheModule() {
        XCTAssertEqual(
            XCTEvidenceMapping.testName(xctestName: "-[Mod.Outer.Inner testX]"),
            "Outer.Inner/testX()")
    }

    func testANameWithoutTheBracketShapeIsKeptAsItIs() {
        XCTAssertEqual(XCTEvidenceMapping.testName(xctestName: "testX"), "testX")
        XCTAssertEqual(XCTEvidenceMapping.testName(xctestName: "-[OnlyClass]"), "-[OnlyClass]")
    }

    func testTheModuleIsTheFirstSegmentOfTheReflectedClassName() {
        XCTAssertEqual(
            XCTEvidenceMapping.moduleName(reflecting: "StacklyAppUITests.ShellUITests"),
            "StacklyAppUITests")
        XCTAssertEqual(XCTEvidenceMapping.moduleName(reflecting: "Lonely"), "Lonely")
    }
}
