import Foundation
import Testing

@testable import StacklyEvidenceCore
@testable import StacklyTestSupport

/// The evidence trait and its scope: one record per test function, with an
/// honest final status (REQ-051).
struct EvidenceTraitTests {
    private struct SampleError: Error {}

    @Test(
        .evidence(
            covers: ["REQ-051"],
            expected:
                "every Swift test that proves a PRD ID leaves one record with an honest final status"
        ))
    func everyScopeLeavesOneRecordWithAnHonestFinalStatus() async throws {
        let passed = try await runScope { _ in }
        #expect(passed.record.status == .pass)
        #expect(passed.record.actual == Record.defaultActual)
        #expect(passed.record.finishedAt != nil)
        #expect(passed.files == [RecordFile.name(of: passed.record.id)])

        let observed = try await runScope { _ in Evidence.actual("ready after 2 probes") }
        #expect(observed.record.status == .pass)
        #expect(observed.record.actual == "ready after 2 probes")

        let failed = try await runScope { _ in Issue.record("a deliberate failure") }
        #expect(failed.record.status == .fail)
        #expect(failed.record.actual?.contains("a deliberate failure") == true)

        let known = try await runScope { _ in withKnownIssue { Issue.record("a known issue") } }
        #expect(known.record.status == .pass)

        let thrown = try await runScope { _ in throw SampleError() }
        #expect(thrown.record.status == .fail)
        #expect(thrown.record.actual?.hasPrefix("Caught error") == true)

        let blocked = try await runScope { _ in
            try Evidence.blocked(reason: "no managed artifact", owner: "@ojiepermana")
        }
        #expect(blocked.record.status == .blocked)
        #expect(blocked.record.reason == "no managed artifact")
        #expect(blocked.record.owner == "@ojiepermana")
        #expect(blocked.record.actual == nil)
    }

    @Test func aNotRunRecordIsOnDiskBeforeTheBodyEnds() async throws {
        let seen = Captured<Record?>(nil)
        let run = try await runScope { folder in
            let records = folder.appending(path: "records")
            let files = try FileManager.default.contentsOfDirectory(atPath: records.path)
            let only = try #require(files.first)
            let record = try JSONDecoder().decode(
                Record.self, from: Data(contentsOf: records.appending(path: only)))
            seen.update { $0 = record }
        }
        let started = try #require(seen.current)
        #expect(started.status == .notRun)
        #expect(started.finishedAt == nil)
        #expect(started.id == run.record.id)
        #expect(run.record.status == .pass)
    }

    @Test func aCancelledTestWithoutBlockedStaysNotRun() async throws {
        let task = Task {
            try await runScope { _ in
                while !Task.isCancelled {
                    await Task.yield()
                }
            }
        }
        task.cancel()
        let run = try await task.value
        #expect(run.record.status == .notRun)
        #expect(run.record.finishedAt == nil)
        #expect(run.issues.isEmpty)
    }

    @Test func blockedWinsOverAFailureNotedBeforeIt() async throws {
        let run = try await runScope { _ in
            Issue.record("a failure before the prerequisite check")
            try Evidence.blocked(reason: "no signing identity", owner: "feature 15 owner")
        }
        #expect(run.record.status == .blocked)
    }

    @Test func blockedWithoutAnOwnerFailsTheRecord() async throws {
        let run = try await runScope { _ in
            try Evidence.blocked(reason: "no managed artifact", owner: " ")
        }
        #expect(run.record.status == .fail)
        #expect(run.record.actual?.contains("owner who can unblock it") == true)
    }

    @Test func recordNamesTheSwiftTestItCameFrom() async throws {
        let run = try await runScope { _ in }
        let record = run.record
        #expect(record.schema == "stackly.evidence/v1")
        #expect(record.method == .automated)
        #expect(record.covers == ["REQ-051"])
        #expect(record.evidencePath == "output.log")
        #expect(record.steps == nil)
        #expect(record.test.language == .swift)
        #expect(record.test.package == nil)
        #expect(record.test.target == "StacklyTestSupportTests")
        #expect(record.test.name == "EvidenceTraitTests/recordNamesTheSwiftTestItCameFrom()")
        #expect(record.test.file?.hasSuffix("/EvidenceTraitTests.swift") == true)
        #expect((record.test.line ?? 0) > 0)
    }

    @Test func subjectsAreRecordedWithTheirDigest() async throws {
        let digest = String(repeating: "0", count: 64)
        let run = try await runScope { _ in
            Evidence.subject(
                provider: "postgresql", version: "18.1", digest: digest, source: .external)
        }
        #expect(run.record.status == .pass)
        #expect(
            run.record.subjects == [
                Subject(provider: "postgresql", version: "18.1", digest: digest, source: .external)
            ])
    }

    @Test func aVersionSelectorAsASubjectFailsTheRecord() async throws {
        let run = try await runScope { _ in
            Evidence.subject(provider: "postgresql", version: "18", digest: nil, source: .managed)
        }
        #expect(run.record.status == .fail)
        #expect(run.record.subjects.isEmpty)
        #expect(run.record.actual?.contains("is not exact") == true)
    }

    @Test func aDigestWithAPrefixFailsTheRecord() async throws {
        let run = try await runScope { _ in
            Evidence.subject(
                provider: "nats", version: "2.15.1", digest: "sha256:abc", source: .managed)
        }
        #expect(run.record.status == .fail)
        #expect(run.record.subjects.isEmpty)
    }

    @Test func aNativeScopeNeverTakesAFixtureSource() async throws {
        let run = try await runScope(declared: .native, run: .native) { _ in
            Evidence.subject(provider: "valkey", version: "8.1.4", digest: nil, source: .fixture)
            Evidence.actual("ready on port 54001")
        }
        #expect(run.record.status == .fail)
        #expect(run.record.subjects.isEmpty)
    }

    @Test func aFixtureTestInANativeRunFailsWithoutRunningItsBody() async throws {
        let plan = RunPlan.make(
            covers: ["REQ-051"], kind: .fixture, expected: "a fixture test",
            environment: [RunPlan.kindVariable: "native"])
        guard case .run(_, let mismatch?) = plan else {
            Issue.record("expected a mismatch, found \(plan)")
            return
        }
        let bodyRan = Captured(false)
        let run = try await runScope(run: .native, mismatch: mismatch) { _ in
            bodyRan.update { $0 = true }
        }
        #expect(!bodyRan.current)
        #expect(run.record.status == .fail)
        #expect(run.record.actual?.contains("NativeTests") == true)
    }

    @Test func emptyActualFailsTheRecord() async throws {
        let run = try await runScope { _ in Evidence.actual("  ") }
        #expect(run.record.status == .fail)
    }

    @Test func evidenceCallsOutsideTheTraitRecordAnIssue() async throws {
        let test = try #require(Test.current)
        let issues = Captured<[String]>([])
        let captureIssues = IssueHandlingTrait.compactMapIssues { issue in
            issues.update { $0.append(String(describing: issue)) }
            return nil
        }
        try await captureIssues.provideScope(for: test, testCase: nil) {
            Evidence.actual("ready")
            Evidence.subject(provider: "nats", version: "2.15.1", digest: nil, source: .managed)
        }
        #expect(issues.current.count == 2)
        #expect(issues.current.allSatisfy { $0.contains("outside a test with the evidence trait") })
        #expect(throws: EvidenceSetupError.self) {
            try Evidence.blocked(reason: "no managed artifact", owner: "@ojiepermana")
        }
    }

    @Test(
        .evidence(
            covers: ["REQ-051"],
            expected: "a test with a second evidence trait records an issue instead of a record"))
    func aSecondEvidenceTraitRecordsAnIssue() async throws {
        let test = try #require(Test.current)
        let issues = Captured<[String]>([])
        let bodyRan = Captured(false)
        let captureIssues = IssueHandlingTrait.compactMapIssues { issue in
            issues.update { $0.append(String(describing: issue)) }
            return nil
        }
        let second = EvidenceTrait(covers: ["REQ-051"], kind: .fixture, expected: "a second one")
        try await captureIssues.provideScope(for: test, testCase: nil) {
            try await second.provideScope(for: test, testCase: nil) {
                bodyRan.update { $0 = true }
            }
        }
        #expect(!bodyRan.current)
        #expect(issues.current.count == 1)
        #expect(issues.current.first?.contains("only one evidence trait per test") == true)
    }

    @Test(arguments: [1, 2])
    func oneScopeCoversEveryCaseOfAParameterizedTest(_ argument: Int) throws {
        let test = try #require(Test.current)
        let testCase = try #require(Test.Case.current)
        let trait = EvidenceTrait(covers: ["REQ-051"], kind: .fixture, expected: "one scope")
        #expect(trait.scopeProvider(for: test, testCase: testCase) == nil)
        #expect(trait.scopeProvider(for: test, testCase: nil) != nil)
    }
}
