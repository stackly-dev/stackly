import Foundation
import Testing

@testable import StacklyEvidenceCore
@testable import StacklyTestSupport

/// The hand written `Codable` writes the `stackly.evidence/v1` shape, and
/// record files are written atomically.
struct RecordTests {
    private func record(status: Status = .notRun, at date: Date = Date()) -> Record {
        Record(
            schema: Record.schemaName, id: .version7(at: date), covers: ["AC-047"],
            kind: .fixture, method: .automated,
            test: TestIdentity(
                language: .swift, package: nil, target: "StacklyClientTests",
                name: "InstanceListTests/clientRefusesAnotherProtocolMajor()",
                file: "/repo/clients/swift/Tests/StacklyClientTests/InstanceListTests.swift",
                line: 7),
            status: status, expected: "the client refuses a daemon with another protocol major",
            actual: nil, reason: nil, owner: nil, subjects: [], steps: nil, reviewer: nil,
            startedAt: date, finishedAt: nil, evidencePath: Record.outputLog)
    }

    private func jsonObject(_ data: Data) throws -> [String: Any] {
        try #require(JSONSerialization.jsonObject(with: data) as? [String: Any])
    }

    @Test func aRecordWritesEveryFieldInCamelCaseWithExplicitNulls() throws {
        let original = record()
        let object = try jsonObject(original.jsonData())

        #expect(
            Set(object.keys) == [
                "schema", "id", "covers", "kind", "method", "test", "status", "expected",
                "actual", "reason", "owner", "subjects", "steps", "reviewer", "startedAt",
                "finishedAt", "evidencePath",
            ])
        #expect(object["status"] as? String == "NOT_RUN")
        #expect(object["kind"] as? String == "fixture")
        #expect(object["actual"] is NSNull)
        #expect(object["finishedAt"] is NSNull)
        #expect(object["id"] as? String == original.id.uuidString.lowercased())
        let test = try #require(object["test"] as? [String: Any])
        #expect(Set(test.keys) == ["language", "package", "target", "name", "file", "line"])
        #expect(test["package"] is NSNull)
        let startedAt = try #require(object["startedAt"] as? String)
        #expect(startedAt.wholeMatch(of: /\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z/) != nil)
    }

    @Test func aRecordRoundTripsThroughJSON() throws {
        // Fractions a binary Double holds exactly, so millisecond text round trips.
        let date = Date(timeIntervalSince1970: 1_792_311_700.125)
        var original = record(status: .pass, at: date)
        original.actual = Record.defaultActual
        original.finishedAt = date.addingTimeInterval(0.25)
        original.subjects = [
            Subject(provider: "nats", version: "2.15.1", digest: nil, source: .managed)
        ]

        let decoded = try JSONDecoder().decode(Record.self, from: original.jsonData())
        #expect(decoded == original)
        #expect(String(decoding: try original.jsonData(), as: UTF8.self).hasSuffix("}\n"))
    }

    @Test func aRecordWithoutOptionalFieldsDecodesWithDefaults() throws {
        let json = """
            {
              "schema": "stackly.evidence/v1",
              "id": "01a15a2f-0a6c-7f38-a5e1-7d8c9b0a1f2e",
              "covers": ["AC-011"],
              "kind": "native",
              "method": "automated",
              "test": { "language": "rust", "name": "tests::stop_never_sends_sigkill" },
              "status": "NOT_RUN",
              "expected": "a normal stop ends the dummy server with SIGTERM only",
              "startedAt": "2026-10-18T10:30:00Z",
              "evidencePath": "output.log",
              "context": { "sourceCommit": "bc87d45" }
            }
            """
        let decoded = try JSONDecoder().decode(Record.self, from: Data(json.utf8))
        #expect(decoded.subjects.isEmpty)
        #expect(decoded.actual == nil)
        #expect(decoded.finishedAt == nil)
        #expect(decoded.test.package == nil)
        #expect(decoded.kind == .native)
        #expect(decoded.id.uuidString.lowercased() == "01a15a2f-0a6c-7f38-a5e1-7d8c9b0a1f2e")
    }

    @Test(arguments: [
        "2026-10-18T10:30:00Z", "2026-10-18T10:30:00.112Z", "2026-10-18T10:30:00.112345678Z",
    ])
    func timestampsParseWithAnyFractionalPrecision(_ text: String) throws {
        let json = #"{"at": "\#(text)"}"#
        struct Stamp: Decodable {
            let at: Date
            enum CodingKeys: String, CodingKey { case at }
            init(from decoder: any Decoder) throws {
                let container = try decoder.container(keyedBy: CodingKeys.self)
                at = try RFC3339.decode(forKey: .at, in: container)
            }
        }
        let stamp = try JSONDecoder().decode(Stamp.self, from: Data(json.utf8))
        let whole = Date(timeIntervalSince1970: 1_792_319_400)
        #expect(stamp.at.timeIntervalSince(whole) >= 0)
        #expect(stamp.at.timeIntervalSince(whole) < 1)
    }

    @Test(arguments: [
        "2026-10-18T08:21:40.112Z", "2026-10-18T08:21:40.999Z", "2026-10-18T08:21:40.000Z",
        "2026-10-18T08:21:59.001Z",
    ])
    func aParsedTimestampFormatsBackToTheSameText(_ text: String) throws {
        let date = try #require(RFC3339.date(from: text))
        #expect(RFC3339.string(from: date) == text)
    }

    @Test func formattingRoundsToTheNearestMillisecond() {
        let date = Date(timeIntervalSince1970: 1_792_319_400.9996)
        #expect(RFC3339.string(from: date) == "2026-10-18T10:30:01.000Z")
    }

    @Test func aTimestampThatIsNotRFC3339FailsToDecode() {
        let json = #"{"at": "18 October 2026"}"#
        struct Stamp: Decodable {
            enum CodingKeys: String, CodingKey { case at }
            init(from decoder: any Decoder) throws {
                let container = try decoder.container(keyedBy: CodingKeys.self)
                _ = try RFC3339.decode(forKey: .at, in: container)
            }
        }
        #expect(throws: DecodingError.self) {
            try JSONDecoder().decode(Stamp.self, from: Data(json.utf8))
        }
    }

    @Test func version7IDsCarryTheTimeTheVersionAndTheVariant() {
        let date = Date(timeIntervalSince1970: 1_792_311_700.112)
        let id = UUID.version7(at: date).uuid
        let milliseconds =
            [id.0, id.1, id.2, id.3, id.4, id.5].reduce(UInt64(0)) { $0 << 8 | UInt64($1) }
        #expect(milliseconds == 1_792_311_700_112)
        #expect(id.6 >> 4 == 7)
        #expect(id.8 >> 6 == 0b10)

        let earlier = UUID.version7(at: date).uuidString.lowercased()
        let later = UUID.version7(at: date.addingTimeInterval(0.002)).uuidString.lowercased()
        #expect(earlier < later)
    }

    @Test func aRecordFileIsWrittenAtomicallyAndReplacedByTheFinalRecord() throws {
        let folder = try TemporaryFolder()
        defer { folder.remove() }
        var written = record()
        try RecordFile.write(written, into: folder.url)
        written.status = .pass
        written.actual = Record.defaultActual
        written.finishedAt = Date()
        try RecordFile.write(written, into: folder.url)

        let records = folder.url.appending(path: "records")
        let files = try FileManager.default.contentsOfDirectory(atPath: records.path)
        #expect(files == [RecordFile.name(of: written.id)])
        let onDisk = try JSONDecoder().decode(
            Record.self, from: Data(contentsOf: records.appending(path: files[0])))
        #expect(onDisk.status == .pass)
    }
}
