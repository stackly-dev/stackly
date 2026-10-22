import Foundation
import StacklyClient
import StacklyTestSupport
import Testing

@testable import StacklyUI

/// The managed runtime cases of the fixtures (spec 0018 AC-18, added to spec 0068): they
/// round trip through the wire coders, line up with the instance and operation cases, and
/// carry only fixed times and IDs.
struct FixtureRuntimeTests {
    @Test func everyRuntimeCaseRoundTripsThroughTheWireCoders() throws {
        func roundTrip<Value: Codable & Equatable>(_ value: Value) throws {
            let data = try StacklyJSON.encoder().encode(value)
            #expect(try StacklyJSON.decoder().decode(Value.self, from: data) == value)
        }
        try roundTrip(FixtureRuntime.page)
        try roundTrip(FixtureRuntime.detail)
        try roundTrip(FixtureRuntime.resolved)
        try roundTrip(FixtureRuntime.installing)
    }

    @Test func theInstalledRuntimesAreManagedAndNumberedLikeTheInstancesThatUseThem() throws {
        #expect(
            FixtureRuntime.summaries.map(\.id)
                == FixtureRuntime.installedNumbers.map {
                    FixtureID.uuid(kind: FixtureID.runtime, number: $0)
                })
        for summary in FixtureRuntime.summaries {
            #expect(summary.installMode == .managed)
            #expect(summary.state == .installed)
            #expect(summary.artifactSha256?.count == 64)
            for user in summary.usedBy ?? [] {
                let instance = try #require(
                    FixtureScenario.instanceCases.first { $0.id == user.instanceId })
                #expect(instance.name == user.name)
                #expect(instance.runtimeVersion == summary.version)
                #expect(instance.runtimeId == summary.id)
            }
        }
        #expect(FixtureRuntime.summaries.last?.usedBy == [])
    }

    @Test func theInstallingRuntimeIsTheResourceOfOperationCaseOne() throws {
        let operation = try #require(FixtureScenario.operationCases.first)
        #expect(operation.kind == .installRuntime)
        let installing = FixtureRuntime.installing
        #expect(installing.operationId == operation.id)
        #expect(
            operation.resource?.id == installing.runtimeId.uuidString.lowercased())
        #expect(
            FixtureRuntime.summaries.allSatisfy { $0.id != installing.runtimeId },
            "a runtime that is still installing is not in the list yet")
        #expect(FixtureRuntime.page.installing == [installing])
    }

    @Test func theDetailIsTheFirstRuntimeAndItsRecordBelongsToTheSameArtifact() throws {
        let detail = FixtureRuntime.detail
        #expect(detail.summary == FixtureRuntime.summaries[0])
        #expect(detail.artifact?.sha256 == detail.summary.artifactSha256)
        #expect(
            detail.compatibility?.allSatisfy { $0.sha256 == detail.summary.artifactSha256 } == true)
        #expect(detail.path.hasPrefix("/fixture/"))
    }

    @Test func everyTimeComesFromTheFixtureClock() {
        for summary in FixtureRuntime.summaries {
            #expect(summary.lastVerifiedAt == FixtureClock.minutesAgo(1))
            let installed = summary.installedAt.map { FixtureClock.now.timeIntervalSince($0) }
            #expect(installed.map { $0.truncatingRemainder(dividingBy: 86_400) } == 0)
        }
        #expect(FixtureRuntime.detail.compatibility?.first?.testedAt == FixtureClock.daysAgo(3))
    }

    @Test func theRepliesAnswerTheThreeReadMethods() {
        #expect(
            Set(FixtureRuntime.replies.keys) == ["runtime.list", "runtime.get", "runtime.resolve"])
        guard case .reply(.runtimeList(let page)) = FixtureRuntime.replies["runtime.list"] else {
            Issue.record("runtime.list does not answer a runtime.list reply")
            return
        }
        #expect(page == FixtureRuntime.page)
    }

    @Test func theCaseNamesNoCurrentContractAndNoRealClock() throws {
        let source = try String(
            contentsOf: URL(filePath: #filePath)
                .deletingLastPathComponent().deletingLastPathComponent()
                .deletingLastPathComponent().deletingLastPathComponent()
                .appending(path: "Sources/StacklyUI/Fixtures/FixtureRuntimeCases.swift"),
            encoding: .utf8)
        #expect(source.hasPrefix("#if DEBUG"))
        #expect(!source.contains("Date.now") && !source.contains("Date()"))
        #expect(!source.contains("ContractInfo"))
    }
}
