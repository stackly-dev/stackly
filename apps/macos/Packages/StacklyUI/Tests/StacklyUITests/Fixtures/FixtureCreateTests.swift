import Foundation
import StacklyClient
import StacklyTestSupport
import Testing

@testable import StacklyUI

/// The create preflight case of the fixtures (spec 0020 AC-2, added to spec 0068): it round
/// trips through the wire coders, names an instance the scenarios list, and carries only
/// fixed values.
struct FixtureCreateTests {
    @Test func thePortConflictRoundTripsThroughTheWireCoders() throws {
        let data = try StacklyJSON.encoder().encode(FixtureCreate.portConflict)
        #expect(
            try StacklyJSON.decoder().decode(CreatePreflight.self, from: data)
                == FixtureCreate.portConflict)
    }

    @Test func thePreflightIsNotReadyExactlyBecauseOneRowIsAnError() throws {
        let preflight = FixtureCreate.portConflict
        #expect(!preflight.ready)
        let failing = preflight.checks.filter { $0.status == .error }
        #expect(failing.map(\.id) == ["endpoint.postgres"])
        let row = try #require(failing.first)
        #expect(row.suggestedPort == FixtureCreate.suggestedPort)
        #expect(row.error?.code == "supervisor.endpoint_conflict")
        #expect(row.error?.fields == preflight.fields)
        #expect(preflight.checks.filter { $0.status != .error }.allSatisfy { $0.error == nil })
    }

    @Test func theRowNamesTheInstanceThatHoldsThePortAndTheRuntimeOfTheScenarios() throws {
        let holder = try #require(
            FixtureScenario.instanceCases.first {
                $0.id
                    == FixtureID.uuid(
                        kind: FixtureID.instance, number: FixtureCreate.portHolderNumber)
            })
        let row = try #require(
            FixtureCreate.portConflict.checks.first { $0.id == "endpoint.postgres" })
        #expect(row.detail == "In use by \(holder.name)")
        #expect(
            holder.runtimeId
                == FixtureID.uuid(kind: FixtureID.runtime, number: FixtureCreate.runtimeNumber))
        #expect(
            FixtureCreate.portConflict.command.contains(
                "--port postgres=\(FixtureCreate.requestedPort)"))
    }

    @Test func theReplyAnswersInstancePreflight() {
        #expect(Set(FixtureCreate.replies.keys) == ["instance.preflight"])
        guard
            case .reply(.instancePreflight(let preflight)) = FixtureCreate.replies[
                "instance.preflight"]
        else {
            Issue.record("instance.preflight does not answer an instance.preflight reply")
            return
        }
        #expect(preflight == FixtureCreate.portConflict)
    }

    @Test func theCaseNamesNoCurrentContractAndNoRealClock() throws {
        let source = try String(
            contentsOf: URL(filePath: #filePath)
                .deletingLastPathComponent().deletingLastPathComponent()
                .deletingLastPathComponent().deletingLastPathComponent()
                .appending(path: "Sources/StacklyUI/Fixtures/FixtureCreateCases.swift"),
            encoding: .utf8)
        #expect(source.hasPrefix("#if DEBUG"))
        #expect(!source.contains("Date.now") && !source.contains("Date()"))
        #expect(!source.contains("ContractInfo"))
    }
}
