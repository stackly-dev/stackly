import Foundation
import StacklyClient
import Testing

@testable import StacklyUI

/// The three states of the Get column and the progress of an install (spec 0018 AC-17): the
/// order in which they win, the join by the digest, the percent, and the stage labels.
struct RuntimeGetCellTests {
    private static let host = HostTarget(os: .macos, arch: .aarch64)
    private static let digest = String(repeating: "ab", count: 32)

    private static func artifact(
        os: TargetOs = .macos, arch: TargetArch = .aarch64, status: ArtifactStatus = .available,
        sha256: String? = digest
    ) -> CatalogArtifact {
        CatalogArtifact(
            id: "x", os: os, arch: arch, status: status, sha256: sha256, minOs: "13.0",
            downloadSize: 1, installedSize: nil, license: nil, licenseUrl: nil, source: nil,
            blockedReason: nil, dependencies: nil)
    }

    private static func version(
        artifacts: [CatalogArtifact], installable: Bool = true, reason: SupportReason? = nil
    ) -> CatalogVersion {
        CatalogVersion(
            version: "2.15.1", line: "2.15", support: nil,
            install: InstallVerdict(installable: installable, reason: reason),
            artifacts: artifacts)
    }

    private func cell(
        _ version: CatalogVersion, installed: RuntimeSummary? = nil,
        installing: InstallingRuntime? = nil, operation: OperationSnapshot? = nil
    ) -> RuntimeGetCell {
        RuntimeGetCell.resolve(
            version: version, host: Self.host, installed: { _ in installed },
            installing: { _ in installing }, operation: { _ in operation })
    }

    @Test func aVersionWithoutAnArtifactForThisMacHasNoAction() {
        #expect(cell(Self.version(artifacts: [])) == .none)
        #expect(cell(Self.version(artifacts: [Self.artifact(arch: .x8664)])) == .none)
        #expect(cell(Self.version(artifacts: [Self.artifact(os: .linux)])) == .none)
        #expect(cell(Self.version(artifacts: [Self.artifact(status: .blocked)])) == .none)
        #expect(cell(Self.version(artifacts: [Self.artifact(sha256: nil)])) == .none)
    }

    @Test func aVersionThatCanBeInstalledShowsGet() {
        let result = cell(Self.version(artifacts: [Self.artifact()]))
        guard case .get(_, let sha256, let reason) = result else {
            Issue.record("expected Get, got \(result)")
            return
        }
        #expect(sha256 == Self.digest)
        #expect(reason == nil)
    }

    @Test func theGateOfTheCatalogDisablesGetWithItsReason() {
        let version = Self.version(
            artifacts: [Self.artifact()], installable: false, reason: .requiresNewerOs)
        guard case .get(_, _, let reason) = cell(version) else {
            Issue.record("expected Get")
            return
        }
        #expect(reason == CatalogCopy.reasonText(.requiresNewerOs))
    }

    @Test func anInstalledRuntimeWinsOverARunningInstall() {
        let version = Self.version(artifacts: [Self.artifact()])
        #expect(
            cell(
                version, installed: FixtureRuntime.summaries[0],
                installing: FixtureRuntime.installing) == .installed)
    }

    @Test func aRunningInstallShowsTheProgressOfItsOperation() {
        let version = Self.version(artifacts: [Self.artifact()])
        let operation = OperationSummary.fixture(
            number: 1, kind: .installRuntime, state: .running, stage: "extract",
            progress: .counted(CountedProgress(done: 1, total: 2)))
        guard
            case .installing(let progress) = cell(
                version, installing: FixtureRuntime.installing, operation: operation)
        else {
            Issue.record("expected the progress of the install")
            return
        }
        // Two stages are done and half of the third: (2 * 2 + 1) / (5 * 2) = 50%.
        #expect(progress.percent == 50)
        #expect(progress.currentTitle == "Extract")
    }

    @Test func anInstallWhoseOperationTheCenterLacksReadsAsQueued() {
        let version = Self.version(artifacts: [Self.artifact()])
        guard
            case .installing(let progress) = cell(
                version, installing: FixtureRuntime.installing)
        else {
            Issue.record("expected the progress of the install")
            return
        }
        #expect(progress.outcome == .waiting)
        #expect(progress.compactText == "Queued")
        #expect(progress.stages.map(\.state) == Array(repeating: .pending, count: 5))
    }

    @Test func theFiveStagesOfAnInstallHaveTheirLabels() {
        #expect(
            RuntimeInstallProgress.stages.map(\.title)
                == ["Download", "Verify digest", "Extract", "Validate binary", "Commit"])
        #expect(
            RuntimeInstallProgress.stages.map(\.key)
                == ["download", "verify_digest", "extract", "validate_binary", "commit"])
    }

    @Test func thePercentOfAnInstallNeverReachesOneHundredBeforeItSucceeded() {
        let commit = OperationSummary.fixture(
            number: 1, kind: .installRuntime, state: .running, stage: "commit",
            progress: .counted(CountedProgress(done: 10, total: 10)))
        #expect(RuntimeInstallProgress.value(operation: commit).percent == 99)
        let done = OperationSummary.fixture(number: 1, kind: .installRuntime, state: .succeeded)
        #expect(RuntimeInstallProgress.value(operation: done).percent == 100)
    }

    @Test func downloadOfTheFirstStageStartsAtZeroAndGrowsWithTheBytes() {
        let none = OperationSummary.fixture(
            number: 1, kind: .installRuntime, state: .running, stage: "download",
            progress: .indeterminate)
        #expect(RuntimeInstallProgress.value(operation: none).percent == 0)
        let half = OperationSummary.fixture(
            number: 1, kind: .installRuntime, state: .running, stage: "download",
            progress: .counted(CountedProgress(done: 50, total: 100)))
        #expect(RuntimeInstallProgress.value(operation: half).percent == 10)
        #expect(
            RuntimeInstallProgress.value(operation: half).compactText == "Download · 10%")
    }

    @Test func theGetHelpIsTheReasonTheButtonCannotRunElseWhatItDoes() {
        func help(_ availability: ActionAvailability) -> String {
            RuntimeGetCell.help(for: availability, displayName: "PostgreSQL", version: "17.11")
        }
        #expect(help(.enabled) == "Install PostgreSQL 17.11")
        #expect(help(.busy) == "Install PostgreSQL 17.11")
        #expect(
            help(.disabled(reason: "stacklyd is unavailable.", showsReason: false))
                == "stacklyd is unavailable.",
            "the mutation gate's reason, not what the button would do")
        #expect(
            help(.disabled(reason: "Not tested on this Mac.", showsReason: true))
                == "Not tested on this Mac.")
    }
}
