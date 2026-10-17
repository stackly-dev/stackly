import Foundation
import Testing

@testable import StacklyTestSupport

/// The shared examples in `schemas/evidence/examples/`, the contract the Rust
/// tests in `stackly-evidence` read too. A name is `<type>-<case>.json` or,
/// for an example that breaks rules on purpose, `invalid-<type>-<case>.json`.
enum SharedExamples {
    /// The examples folder, found from this file's path. Builds must never
    /// remap source paths (`-file-prefix-map`), or this breaks.
    static let folder = URL(filePath: #filePath)
        .deletingLastPathComponent()  // SharedExamplesTests.swift
        .deletingLastPathComponent()  // StacklyTestSupportTests
        .deletingLastPathComponent()  // Tests
        .deletingLastPathComponent()  // swift
        .deletingLastPathComponent()  // clients
        .appending(path: "schemas/evidence/examples", directoryHint: .isDirectory)

    /// True when the examples folder exists.
    static var isPresent: Bool {
        FileManager.default.fileExists(atPath: folder.path)
    }

    /// Every example file name whose type is `type`, sorted.
    static func names(ofType type: String) throws -> [String] {
        try FileManager.default.contentsOfDirectory(atPath: folder.path)
            .filter { name in
                let unprefixed = name.hasPrefix("invalid-") ? String(name.dropFirst(8)) : name
                return unprefixed.hasPrefix(type + "-") && name.hasSuffix(".json")
            }
            .sorted()
    }

    /// The raw bytes of one example.
    static func data(_ name: String) throws -> Data {
        try Data(contentsOf: folder.appending(path: name))
    }

    /// One example as a JSON object.
    static func object(_ name: String) throws -> [String: Any] {
        try #require(JSONSerialization.jsonObject(with: data(name)) as? [String: Any])
    }

    /// One example decoded as a record.
    static func record(_ name: String) throws -> Record {
        try JSONDecoder().decode(Record.self, from: data(name))
    }
}

// provisional: F2. The examples come with the `evidence check` milestone,
// built on its own branch; drop this condition once that branch is merged.
// The Rust examples test fails when the folder is missing, so after the
// merge this condition cannot hide a lost folder.
@Suite(
    .enabled(
        if: SharedExamples.isPresent,
        "schemas/evidence/examples/ is not on this branch yet (evidence check milestone)"))
struct SharedExamplesTests {
    @Test func everyRecordExampleDecodes() throws {
        let names = try SharedExamples.names(ofType: "record")
        #expect(names.contains { $0.hasPrefix("record-swift-") })
        #expect(names.contains { $0.hasPrefix("record-rust-") })
        #expect(names.contains { $0.hasPrefix("record-manual-") })
        for name in names {
            let record = try SharedExamples.record(name)
            #expect(record.schema == Record.schemaName, "\(name)")
        }
    }

    @Test func everyRecordInsideAReportExampleDecodes() throws {
        let names = try SharedExamples.names(ofType: "report")
        #expect(!names.isEmpty)
        for name in names {
            let records = try #require(SharedExamples.object(name)["records"] as? [Any])
            #expect(!records.isEmpty, "\(name)")
            for record in records {
                let data = try JSONSerialization.data(withJSONObject: record)
                _ = try JSONDecoder().decode(Record.self, from: data)
            }
        }
    }

    @Test(
        .evidence(
            covers: ["REQ-051"],
            expected:
                "the Swift trait writes the same stackly.evidence/v1 record shape as the shared examples"
        ))
    func swiftRecordExamplesMatchWhatTheTraitWrites() throws {
        let names = try SharedExamples.names(ofType: "record").filter {
            $0.hasPrefix("record-swift-")
        }
        #expect(!names.isEmpty)
        for name in names {
            let example = try SharedExamples.object(name)
            let decoded = try SharedExamples.record(name)
            #expect(decoded.test.language == .swift, "\(name)")
            #expect(decoded.test.package == nil, "\(name)")
            #expect(decoded.test.target != nil, "\(name)")

            let written = try decoded.jsonData()
            let object = try #require(
                JSONSerialization.jsonObject(with: written) as? [String: Any])
            #expect(Set(object.keys) == Set(example.keys).subtracting(["context"]), "\(name)")
            let exampleTest = try #require(example["test"] as? [String: Any])
            let writtenTest = try #require(object["test"] as? [String: Any])
            #expect(Set(writtenTest.keys) == Set(exampleTest.keys), "\(name)")

            let reread = try JSONDecoder().decode(Record.self, from: written)
            #expect(reread == decoded, "\(name)")
        }
    }

    @Test func validRecordExamplesPassTheSwiftArgumentAndSubjectRules() throws {
        let names = try SharedExamples.names(ofType: "record").filter {
            !$0.hasPrefix("invalid-")
        }
        for name in names {
            let record = try SharedExamples.record(name)
            #expect(
                Rules.argumentViolations(covers: record.covers, expected: record.expected)
                    .isEmpty,
                "\(name)")
            for subject in record.subjects {
                #expect(Rules.isExactVersion(subject.version), "\(name)")
                #expect(subject.digest.map(Rules.isSHA256Hex) ?? true, "\(name)")
                #expect(!(record.kind == .native && subject.source == .fixture), "\(name)")
            }
        }
    }

    @Test func swiftRulesRejectTheWrongCoversAndTheVersionSelectorExamples() throws {
        let wrongCovers = try SharedExamples.record("invalid-record-covers-not-prd-id.json")
        #expect(
            !Rules.argumentViolations(covers: wrongCovers.covers, expected: wrongCovers.expected)
                .isEmpty)

        let selector = try SharedExamples.record("invalid-record-native-subject-selector.json")
        #expect(selector.subjects.contains { !Rules.isExactVersion($0.version) })
    }
}
