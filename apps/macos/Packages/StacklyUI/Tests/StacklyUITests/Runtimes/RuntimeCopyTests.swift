import Foundation
import StacklyClient
import Testing

@testable import StacklyUI

/// reads, the status table, and the facts of the inspector.
struct RuntimeCopyTests {
    private let postgres = FixtureRuntime.summaries[0]
    private let traefik = FixtureRuntime.summaries[3]

    // MARK: - Row

    @Test func aRowNamesTheRuntimeAndItsArchitectureAndVerification() {
        #expect(RuntimeCopy.name(of: postgres) == "PostgreSQL 17.6")
        #expect(RuntimeCopy.secondLine(of: postgres) == "arm64 · verified")
        #expect(RuntimeCopy.arch(.aarch64) == "arm64")
        #expect(RuntimeCopy.arch(.x8664) == "x86_64")
        #expect(RuntimeCopy.arch(.unrecognized) == "unknown")
    }

    @Test func aRuntimeThatIsNotInstalledShowsTheLabelOfItsStateOnTheSecondLine() {
        let drifted = RuntimeSummary(
            id: postgres.id, revision: 2, software: "postgresql", displayName: "PostgreSQL",
            version: "17.6", line: "17", target: postgres.target, installMode: .managed,
            state: .drifted, artifactSha256: postgres.artifactSha256, installedAt: nil,
            lastVerifiedAt: nil, support: .experimental, supportReason: .runtimeTested,
            usedBy: [], dependentRuntimes: [])
        #expect(RuntimeCopy.secondLine(of: drifted) == "Drifted")
    }

    @Test func theDigestShowsEightDigitsAnEllipsisAndTheLastFour() {
        let digest = "0123456789abcdef" + String(repeating: "0", count: 44) + "cafe"
        #expect(RuntimeCopy.shortDigest(digest) == "sha256:01234567…cafe")
        #expect(RuntimeCopy.shortDigest(nil) == "—")
        #expect(RuntimeCopy.shortDigest("abc") == "sha256:abc")
    }

    @Test func usedByListsTheInstancesAndMarksARetainedOne() {
        let users = [
            RuntimeUser(instanceId: UUID(), name: "pg17-dev", retained: false),
            RuntimeUser(instanceId: UUID(), name: "old", retained: true),
        ]
        #expect(RuntimeCopy.usedBy(users) == "pg17-dev, old (retained)")
        #expect(RuntimeCopy.usedBy([]) == "Unused")
        #expect(RuntimeCopy.usedBy(nil) == "Unused")
    }

    @Test func removeIsLockedForTheInstancesThatUseTheRuntimeOrForALaterBuild() {
        #expect(RuntimeCopy.removeReason(of: postgres) == "Used by 1 instance")
        #expect(RuntimeCopy.removeReason(of: traefik) == RuntimeCopy.removeLater)
        let two = RuntimeSummary(
            id: UUID(), revision: 1, software: "x", displayName: "X", version: "1", line: "1",
            target: postgres.target, installMode: .managed, state: .installed,
            artifactSha256: nil, installedAt: nil, lastVerifiedAt: nil, support: .unverified,
            supportReason: .untested,
            usedBy: [
                RuntimeUser(instanceId: UUID(), name: "a", retained: false),
                RuntimeUser(instanceId: UUID(), name: "b", retained: true),
            ], dependentRuntimes: [])
        #expect(RuntimeCopy.removeReason(of: two) == "Used by 2 instances")
    }

    @Test func theTitleOfAnInstallNamesTheSoftwareAndTheVersion() {
        #expect(
            RuntimeCopy.installTitle(FixtureRuntime.installing) == "Install PostgreSQL 18.1")
    }

    // MARK: - Inspector

    @Test func theSupportRowSaysTheStatusAndItsReason() {
        #expect(
            RuntimeCopy.supportText(of: postgres)
                == "Experimental: The runtime ran on this Mac; the provider is not tested with it yet."
        )
    }

    @Test func theRecordLineNamesTheHostTheDateAndTheChecks() throws {
        let record = try #require(FixtureRuntime.detail.compatibility?.first)
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = TimeZone(identifier: "UTC") ?? .gmt
        let text = RuntimeCopy.recordText(
            record, locale: Locale(identifier: "en_US"), calendar: calendar,
            timeZone: TimeZone(identifier: "UTC") ?? .gmt)
        #expect(
            text == "Tested on macOS 27.0 (26A428), arm64, Oct 15, 2026: launch_agent, ready, stop")
    }

    @Test func onlyATestedSupportShowsTheRecordThatDecidedIt() throws {
        let records = try #require(FixtureRuntime.detail.compatibility)
        func summary(_ reason: SupportReason) -> RuntimeSummary {
            RuntimeSummary(
                id: postgres.id, revision: 1, software: "postgresql", displayName: "PostgreSQL",
                version: "17.6", line: "17", target: postgres.target, installMode: .managed,
                state: .installed, artifactSha256: postgres.artifactSha256, installedAt: nil,
                lastVerifiedAt: nil, support: .unverified, supportReason: reason, usedBy: [],
                dependentRuntimes: [])
        }
        #expect(RuntimeCopy.decidingRecord(of: summary(.providerTested), in: records) == records[0])
        #expect(RuntimeCopy.decidingRecord(of: summary(.runtimeTested), in: records) == records[0])
        for reason in [SupportReason.untested, .fixtureOnly, .requiresNewerOs, .noBuildForArch] {
            #expect(RuntimeCopy.decidingRecord(of: summary(reason), in: records) == nil)
        }
        #expect(RuntimeCopy.decidingRecord(of: summary(.runtimeTested), in: nil) == nil)
    }

    @Test func theSignatureRowSaysTheKindsAndTheMachOFiles() throws {
        let signature = try #require(FixtureRuntime.detail.artifact?.signature)
        #expect(RuntimeCopy.signatureText(signature) == "Ad hoc · 210 Mach-O files")
        let none = SignatureInfo(
            policy: .adhocPipeline, adhocSignedAtInstall: true, machOFiles: 1, executables: [])
        #expect(RuntimeCopy.signatureText(none) == "Ad hoc · 1 Mach-O file")
    }

    @Test func aDependencyShowsItsVersionKindAndLicense() throws {
        let dependencies = try #require(FixtureRuntime.detail.artifact?.dependencies)
        #expect(
            RuntimeCopy.dependencyText(dependencies[0])
                == "libssl.3.dylib 3.5.9 · bundled · Apache-2.0")
        #expect(RuntimeCopy.dependencyText(dependencies[1]) == "libSystem.B.dylib · macOS")
    }

    @Test func theFootnoteIsTheMockupNote() {
        #expect(
            RuntimeCopy.footnote
                == "Runtimes in use can’t be removed, even by stopped instances. External runtimes are read-only and never upgraded by Stackly."
        )
    }
}
