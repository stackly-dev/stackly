import Foundation
import StacklyClient
import Testing

/// Runs every Swift case in `protocols/fixtures/ipc/index.json`, the same
/// index the Rust tests read (spec 0006 AC-8, AC-10). Cases compare decoded
/// values, never JSON text.
struct IpcFixtureTests {
    @Test(arguments: try FixtureCase.swiftCases())
    func fixtureBehavesAsTheIndexExpects(_ fixture: FixtureCase) throws {
        let data = try Fixture.data("ipc/\(fixture.file)")
        switch fixture.type {
        case "Request":
            try check(Request.self, fixture, data)
        case "ServerFrame":
            try check(ServerFrame.self, fixture, data)
        case "SecretInput":
            try check(SecretInput.self, fixture, data)
        case "OperationAccepted":
            try check(OperationAccepted.self, fixture, data)
        case "NeedsSecretBlocker":
            try check(NeedsSecretBlocker.self, fixture, data)
        case "InstanceSummary":
            try check(InstanceSummary.self, fixture, data)
        case "InstanceBlocker":
            try check(InstanceBlocker.self, fixture, data)
        case "SettingsDocument":
            try check(JSONObject.self, fixture, data)
        default:
            Issue.record("no Swift decoder for type \(fixture.type)")
        }
    }

    @Test func indexHasSwiftCasesOfEveryExpectation() throws {
        let expectations = Set(try FixtureCase.swiftCases().map(\.expect))
        #expect(expectations == ["roundtrip", "decode", "skipped", "reject"])
    }

    @Test func everyCallVariantHasARoundTripFixture() throws {
        let methods = try FixtureCase.swiftCases()
            .filter { $0.type == "Request" && $0.expect == "roundtrip" }
            .map { try Fixture.ipc(Request.self, $0.file).call }
        #expect(methods.contains { if case .daemonHandshake = $0 { true } else { false } })
        #expect(methods.contains(.daemonGetInfo))
        #expect(methods.contains(.daemonGetHealth))
        #expect(methods.contains(.daemonGetCapabilities))
        #expect(methods.contains { if case .daemonShutdown = $0 { true } else { false } })
        #expect(methods.contains { if case .eventsSubscribe = $0 { true } else { false } })
        #expect(methods.contains { if case .eventsUnsubscribe = $0 { true } else { false } })
        #expect(methods.contains { if case .operationGet = $0 { true } else { false } })
        #expect(methods.contains { if case .operationList = $0 { true } else { false } })
        #expect(methods.contains { if case .operationCancel = $0 { true } else { false } })
        #expect(methods.contains { if case .operationResume = $0 { true } else { false } })
        #expect(methods.contains { if case .operationAbandon = $0 { true } else { false } })
        #expect(methods.contains(.operationGetStats))
        #expect(methods.contains(.catalogListProviders))
        #expect(methods.contains { if case .catalogGetProvider = $0 { true } else { false } })
        #expect(methods.contains { if case .secretList = $0 { true } else { false } })
        #expect(methods.contains { if case .secretStatus = $0 { true } else { false } })
        #expect(methods.contains { if case .secretSet = $0 { true } else { false } })
        #expect(methods.contains { if case .secretReveal = $0 { true } else { false } })
        #expect(methods.contains { if case .secretDelete = $0 { true } else { false } })
        #expect(methods.contains { if case .instanceCreate = $0 { true } else { false } })
        #expect(methods.contains { if case .instanceStart = $0 { true } else { false } })
        #expect(methods.contains { if case .instanceStop = $0 { true } else { false } })
        #expect(methods.contains { if case .instanceGet = $0 { true } else { false } })
        #expect(methods.contains { if case .instanceList = $0 { true } else { false } })
        #expect(methods.contains(.storageGetCapacity))
        #expect(methods.contains { if case .runtimeInstall = $0 { true } else { false } })
        #expect(methods.contains { if case .runtimeResolve = $0 { true } else { false } })
        #expect(methods.contains { if case .runtimeList = $0 { true } else { false } })
        #expect(methods.contains { if case .runtimeGet = $0 { true } else { false } })
        #expect(methods.contains { if case .instanceGetCreateForm = $0 { true } else { false } })
        #expect(methods.contains { if case .instancePreflight = $0 { true } else { false } })
        #expect(methods.contains { if case .instanceGetDuplicateForm = $0 { true } else { false } })
        #expect(Set(methods.map(\.methodName)).count == 33)
    }

    private func check<Value: Codable & Equatable>(
        _ type: Value.Type, _ fixture: FixtureCase, _ data: Data
    ) throws {
        let decoder = StacklyJSON.decoder()
        switch fixture.expect {
        case "roundtrip":
            let decoded = try decoder.decode(type, from: data)
            let encoded = try StacklyJSON.encoder().encode(decoded)
            #expect(try decoder.decode(type, from: encoded) == decoded)
        case "decode":
            _ = try decoder.decode(type, from: data)
        case "skipped":
            let probe = try decoder.decode(EventFrameProbe.self, from: data)
            #expect(probe.type == "event")
            #expect(throws: DecodingError.self) { try decoder.decode(type, from: data) }
        case "reject":
            #expect(throws: (any Error).self) { try decoder.decode(type, from: data) }
        default:
            Issue.record("unknown expectation \(fixture.expect)")
        }
    }
}

/// The first two decode stages of a daemon frame: the frame type, then the
/// `EventProbe` that lets a client skip an event kind it does not know.
private struct EventFrameProbe: Decodable {
    let type: String
    let content: EventProbe
}
