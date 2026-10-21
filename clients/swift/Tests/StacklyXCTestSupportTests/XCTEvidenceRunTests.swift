import Foundation
import StacklyEvidenceCore
import Synchronization
import XCTest

@testable import StacklyXCTestSupport

/// The environment a probe test passes to `evidence`, so these tests never
/// set a process environment variable.
private let probeEnvironment = Mutex<[String: String]>([:])

/// A test case whose methods the tests below run by hand. The names do not
/// start with `test`, so XCTest never runs them by itself.
@MainActor
final class Probe: XCTestCase {
    func passes() throws {
        try evidence(
            covers: ["AC-024"], expected: "the probe passes",
            environment: probeEnvironment.withLock { $0 })
    }

    func passesWithActual() throws {
        try evidence(
            covers: ["AC-024"], expected: "the probe passes",
            environment: probeEnvironment.withLock { $0 })
        XCTEvidence.actual("footer read Status: Connected")
    }

    func blocksWithReason() throws {
        try evidence(
            covers: ["AC-024"], expected: "the probe blocks",
            environment: probeEnvironment.withLock { $0 })
        try XCTEvidence.blocked(reason: "no GUI login session", owner: "user")
    }

    func skipsWithoutBlocked() throws {
        try evidence(
            covers: ["AC-024"], expected: "the probe skips",
            environment: probeEnvironment.withLock { $0 })
        throw XCTSkip("not today")
    }

    func callsEvidenceWithoutRun() throws {
        try evidence(covers: ["AC-024"], expected: "the probe passes", environment: [:])
    }
}

/// Runs probes through real XCTest, so the observer sees real test runs and
/// writes real records (spec 0068, AC-10).
@MainActor
final class XCTEvidenceRunTests: XCTestCase {
    private var folder: URL?

    override func setUp() async throws {
        let url = FileManager.default.temporaryDirectory.appending(
            path: "stackly-xct-evidence-\(UUID().uuidString)", directoryHint: .isDirectory)
        try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        folder = url
        probeEnvironment.withLock {
            $0 = [
                RunPlan.directoryVariable: url.path(percentEncoded: false),
                RunPlan.kindVariable: "fixture",
            ]
        }
    }

    override func tearDown() async throws {
        probeEnvironment.withLock { $0 = [:] }
        if let folder {
            try? FileManager.default.removeItem(at: folder)
        }
    }

    /// Runs `selector` of `Probe` and returns the only record it left.
    private func run(_ selector: Selector) throws -> Record {
        let probe = Probe(selector: selector)
        probe.run()
        return try onlyRecord()
    }

    private func onlyRecord() throws -> Record {
        let records = try XCTUnwrap(folder).appending(path: "records", directoryHint: .isDirectory)
        let files = try FileManager.default.contentsOfDirectory(atPath: records.path)
        XCTAssertEqual(files.count, 1, "records: \(files)")
        let name = try XCTUnwrap(files.first)
        let data = try Data(contentsOf: records.appending(path: name))
        return try JSONDecoder().decode(Record.self, from: data)
    }

    func testAPassingTestLeavesAPassRecordWithTheTestIdentity() throws {
        let record = try run(#selector(Probe.passes))
        XCTAssertEqual(record.status, .pass)
        XCTAssertEqual(record.actual, "all expectations passed")
        XCTAssertEqual(record.kind, .fixture)
        XCTAssertEqual(record.method, .automated)
        XCTAssertEqual(record.covers, ["AC-024"])
        XCTAssertEqual(record.expected, "the probe passes")
        XCTAssertEqual(record.test.language, .swift)
        XCTAssertEqual(record.test.target, "StacklyXCTestSupportTests")
        XCTAssertEqual(record.test.name, "Probe/passes()")
        XCTAssertNotNil(record.test.file)
        XCTAssertNotNil(record.finishedAt)
        XCTAssertEqual(record.schema, "stackly.evidence/v1")
    }

    func testAnObservedValueBecomesTheActualOfThePassRecord() throws {
        let record = try run(#selector(Probe.passesWithActual))
        XCTAssertEqual(record.status, .pass)
        XCTAssertEqual(record.actual, "footer read Status: Connected")
    }

    func testBlockedLeavesABlockedRecordWithReasonAndOwner() throws {
        let record = try run(#selector(Probe.blocksWithReason))
        XCTAssertEqual(record.status, .blocked)
        XCTAssertEqual(record.reason, "no GUI login session")
        XCTAssertEqual(record.owner, "user")
        XCTAssertNil(record.actual)
        XCTAssertNotNil(record.finishedAt)
    }

    func testASkipWithoutBlockedLeavesTheNotRunRecord() throws {
        let record = try run(#selector(Probe.skipsWithoutBlocked))
        XCTAssertEqual(record.status, .notRun)
        XCTAssertNil(record.finishedAt)
    }

    func testWithoutARunNothingIsWritten() throws {
        probeEnvironment.withLock { $0 = [:] }
        let probe = Probe(selector: #selector(Probe.callsEvidenceWithoutRun))
        probe.run()
        let records = try XCTUnwrap(folder).appending(path: "records", directoryHint: .isDirectory)
        XCTAssertFalse(FileManager.default.fileExists(atPath: records.path))
    }
}
