import Foundation
import Testing

@testable import StacklyTestSupport

/// What the trait decides from its arguments and the evidence environment.
/// Every case passes its own environment dictionary; no test sets an
/// environment variable.
struct RunPlanTests {
    private static let directory = RunPlan.directoryVariable
    private static let kind = RunPlan.kindVariable

    private func plan(
        _ declared: Evidence.Kind, _ environment: [String: String],
        covers: [String] = ["AC-011"], expected: String = "a plan test"
    ) -> RunPlan {
        RunPlan.make(
            covers: covers, kind: declared, expected: expected, environment: environment)
    }

    @Test func runTargetIsEmptyWithoutARunFolder() throws {
        #expect(try RunPlan.runTarget(in: [:]) == nil)
        #expect(try RunPlan.runTarget(in: [Self.kind: "native"]) == nil)
    }

    @Test func runTargetRejectsARelativeFolderAMissingFolderOrNoKind() throws {
        let folder = try TemporaryFolder()
        defer { folder.remove() }
        let missing = folder.url.appending(path: "missing").path
        let rejected: [[String: String]] = [
            [Self.directory: "relative/run", Self.kind: "native"],
            [Self.directory: missing, Self.kind: "native"],
            [Self.directory: folder.url.path],
            [Self.directory: folder.url.path, Self.kind: ""],
            [Self.directory: folder.url.path, Self.kind: "ci"],
        ]
        for environment in rejected {
            #expect(throws: EvidenceSetupError.self) {
                try RunPlan.runTarget(in: environment)
            }
        }
    }

    @Test func runTargetReadsAnAbsoluteFolderAndItsKind() throws {
        let folder = try TemporaryFolder()
        defer { folder.remove() }
        let target = try RunPlan.runTarget(in: [
            Self.directory: folder.url.path, Self.kind: "fixture",
        ])
        #expect(target?.directory.path == folder.url.path)
        #expect(target?.kind == .fixture)
    }

    @Test func badArgumentsAreRejectedBeforeAnythingElse() {
        let badCovers = plan(.fixture, [:], covers: ["OQ-011", "AC-011", "AC-011"])
        guard case .reject(let message) = badCovers else {
            Issue.record("expected a rejection, found \(badCovers)")
            return
        }
        #expect(message.contains("`OQ-011` is not a PRD ID"))
        #expect(message.contains("covers lists `AC-011` twice"))

        #expect(plan(.native, [:], covers: []) != .run(target: nil, mismatch: nil))
        guard case .reject = plan(.native, [:], covers: []) else {
            Issue.record("a native test with no covers must be rejected, not cancelled")
            return
        }
        guard case .reject = plan(.fixture, [:], expected: " ") else {
            Issue.record("an empty expected sentence must be rejected")
            return
        }
    }

    @Test func aBrokenRunEnvironmentIsRejected() {
        guard case .reject(let message) = plan(.fixture, [Self.directory: "relative/run"]) else {
            Issue.record("a relative run folder must be rejected")
            return
        }
        #expect(message.contains("must be an absolute path"))
    }

    @Test func aNativeTestIsCancelledOutsideANativeRun() throws {
        let folder = try TemporaryFolder()
        defer { folder.remove() }
        let outside: [[String: String]] = [
            [:],
            [Self.kind: "fixture"],
            [Self.directory: folder.url.path, Self.kind: "fixture"],
        ]
        for environment in outside {
            guard case .cancelNative(let message) = plan(.native, environment) else {
                Issue.record("expected a cancel for \(environment)")
                continue
            }
            #expect(message.contains("STACKLY_EVIDENCE_KIND=native"))
        }
    }

    @Test func aNativeTestRunsInANativeRun() throws {
        let folder = try TemporaryFolder()
        defer { folder.remove() }
        #expect(plan(.native, [Self.kind: "native"]) == .run(target: nil, mismatch: nil))
        let inRun = plan(.native, [Self.directory: folder.url.path, Self.kind: "native"])
        guard case .run(let target?, nil) = inRun else {
            Issue.record("expected a native run with a target, found \(inRun)")
            return
        }
        #expect(target.directory.path == folder.url.path)
        #expect(target.kind == .native)
    }

    @Test func aFixtureTestInANativeRunGetsAMismatch() {
        guard case .run(nil, let mismatch?) = plan(.fixture, [Self.kind: "native"]) else {
            Issue.record("a fixture test in a native run must carry a mismatch")
            return
        }
        #expect(mismatch.contains("NativeTests"))
    }

    @Test func aFixtureTestWithoutARunWritesNothing() {
        #expect(plan(.fixture, [:]) == .run(target: nil, mismatch: nil))
    }
}
