import Foundation
import StacklyTestSupport
import Synchronization
import Testing

/// `FixtureCredential` is the Swift twin of `stackly_evidence::FixtureCredential` (spec
/// 0011 AC-18, spec 0002 *Fixture credential*).
struct FixtureCredentialTests {
    @Test func theValueIsThePrefixAnd32LowercaseHexCharacters() throws {
        let value = FixtureCredential().value
        #expect(value.hasPrefix("stackly-fixture-credential-"))
        let random = value.dropFirst(FixtureCredential.prefix.count)
        #expect(random.count == 32)
        #expect(random.allSatisfy { $0.isHexDigit && !$0.isUppercase })
    }

    @Test func twoCredentialsDiffer() {
        #expect(FixtureCredential().value != FixtureCredential().value)
    }

    @Test func itPrintsAsAMaskInEveryTextForm() {
        let credential = FixtureCredential()
        #expect(credential.description == "FixtureCredential(****)")
        #expect(credential.debugDescription == "FixtureCredential(****)")
        #expect("\(credential)" == "FixtureCredential(****)")
        var dumped = ""
        dump(credential, to: &dumped)
        #expect(!dumped.contains(credential.value))
    }

    @Test func assertAbsentPassesForTextWithoutThePrefix() {
        FixtureCredential.assertAbsent(in: "daemon started, nothing secret here", what: "a log")
        FixtureCredential().assertAbsent(in: "", what: "an empty log")
    }

    @Test func assertAbsentNamesWhatLeakedAndNeverTheValue() {
        let credential = FixtureCredential()
        let issues = Mutex<[String]>([])
        withKnownIssue {
            credential.assertAbsent(in: "line: \(credential.value)", what: "the daemon log")
        } matching: { issue in
            issues.withLock { $0.append(issue.comments.map(\.rawValue).joined()) }
            return true
        }
        let recorded = issues.withLock { $0 }.joined()
        #expect(recorded.contains("fixture credential found in the daemon log"))
        #expect(!recorded.contains(credential.value))
    }

    @Test func thePrefixIsTheSameConstantAsInRust() throws {
        let rust = try String(
            contentsOf: Self.repositoryRoot.appending(path: "crates/evidence/src/credential.rs"),
            encoding: .utf8)
        let line = try #require(
            rust.split(separator: "\n").first {
                $0.contains("pub const FIXTURE_CREDENTIAL_PREFIX")
            })
        #expect(line.contains("\"\(FixtureCredential.prefix)\""))
    }

    private static let repositoryRoot = URL(filePath: #filePath)
        .deletingLastPathComponent()  // FixtureCredentialTests.swift
        .deletingLastPathComponent()  // StacklyTestSupportTests
        .deletingLastPathComponent()  // Tests
        .deletingLastPathComponent()  // swift
        .deletingLastPathComponent()  // clients
}
