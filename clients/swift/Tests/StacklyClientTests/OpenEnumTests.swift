import Foundation
import StacklyClient
import Testing

/// Values a newer daemon may send in an open enum decode as `.unrecognized`
/// instead of failing (spec 0006 AC-10), through `OpenEnums.swift`.
struct OpenEnumTests {
    @Test func foreignClientKindDecodesAsUnrecognized() throws {
        let request = try Fixture.ipc(Request.self, "request-handshake-newer-client.json")
        guard case .daemonHandshake(let params) = request.call else {
            Issue.record("expected a handshake")
            return
        }
        #expect(params.client.kind == .unrecognized)
        #expect(params.protocol.minor == 99)
    }

    @Test func foreignHealthAndSubsystemStatusesDecodeAsUnrecognized() throws {
        let frame = try Fixture.ipc(ServerFrame.self, "frame-response-health-unknown-statuses.json")
        guard case .response(let response) = frame,
            case .daemonGetHealth(let health) = response.reply
        else {
            Issue.record("expected a health reply")
            return
        }
        #expect(health.status == .unrecognized)
        #expect(health.subsystems?.first?.status == .unrecognized)
    }

    @Test func foreignGapAndShutdownReasonsDecodeAsUnrecognized() throws {
        let gap = try Fixture.ipc(ServerFrame.self, "frame-event-gap-unknown-reason.json")
        guard case .event(let gapEvent) = gap, case .eventsGap(let content) = gapEvent.kind else {
            Issue.record("expected a gap event")
            return
        }
        #expect(content.reason == .unrecognized)

        let shutdown = try Fixture.ipc(ServerFrame.self, "frame-event-shutdown-unknown-reason.json")
        guard case .event(let shutdownEvent) = shutdown,
            case .daemonShuttingDown(let reason) = shutdownEvent.kind
        else {
            Issue.record("expected a shutdown event")
            return
        }
        #expect(reason.reason == .unrecognized)
    }

    @Test func foreignResourceKindDecodesAsUnrecognized() throws {
        let frame = try Fixture.ipc(ServerFrame.self, "frame-error-unknown-resource-kind.json")
        guard case .error(let error) = frame else {
            Issue.record("expected an error frame")
            return
        }
        #expect(error.error.resource?.kind == .unrecognized)
        #expect(error.error.category == .notFound)
    }

    @Test(arguments: ["\"cli\"", "\"gui\""])
    func knownClientKindsStillDecode(_ json: String) throws {
        let kind = try StacklyJSON.decoder().decode(ClientKind.self, from: Data(json.utf8))
        #expect(kind != .unrecognized)
    }

    @Test func snakeCaseValuesMapToTheirCases() throws {
        let decoder = StacklyJSON.decoder()
        let status = try decoder.decode(SubsystemStatus.self, from: Data("\"not_built\"".utf8))
        #expect(status == .notBuilt)
        let reason = try decoder.decode(GapReason.self, from: Data("\"cursor_expired\"".utf8))
        #expect(reason == .cursorExpired)
    }

    @Test func closedErrorCategoryRejectsAForeignValue() {
        #expect(throws: DecodingError.self) {
            try StacklyJSON.decoder().decode(ErrorCategory.self, from: Data("\"maybe\"".utf8))
        }
    }
}
