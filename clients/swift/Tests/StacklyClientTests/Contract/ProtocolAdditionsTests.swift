import Foundation
import StacklyClient
import Testing

/// What protocol 1.2 added, read from the shared fixtures (spec 0006 AC-8,
/// AC-10, AC-18, AC-19): the client's `interactive` flag, the registry
/// schema and recovery report, error items, and the manager resource.
struct ProtocolAdditionsTests {
    static func handshake(_ file: String) throws -> HandshakeParams {
        let request = try Fixture.ipc(Request.self, file)
        guard case .daemonHandshake(let params) = request.call else {
            throw FixtureShapeError.unexpected(file)
        }
        return params
    }

    static func reply(_ file: String) throws -> Reply {
        guard case .response(let response) = try Fixture.ipc(ServerFrame.self, file) else {
            throw FixtureShapeError.unexpected(file)
        }
        return response.reply
    }

    static func error(_ file: String) throws -> ErrorBody {
        guard case .error(let frame) = try Fixture.ipc(ServerFrame.self, file) else {
            throw FixtureShapeError.unexpected(file)
        }
        return frame.error
    }

    @Test func aClientWithoutTheInteractiveFlagDecodesAsNotSayingIt() throws {
        #expect(
            try Self.handshake("request-handshake-without-interactive.json").client.interactive
                == nil)
        #expect(try Self.handshake("request-handshake.json").client.interactive == true)
    }

    @Test func infoCarriesTheRegistrySchemaAndAnOlderDaemonDecodesWithout() throws {
        guard case .daemonGetInfo(let info) = try Self.reply("frame-response-get-info.json"),
            case .daemonGetInfo(let older) = try Self.reply(
                "frame-response-get-info-older-daemon.json")
        else {
            Issue.record("expected info replies")
            return
        }
        #expect(info.registrySchema == 1)
        #expect(older.registrySchema == nil)
    }

    @Test func healthInRecoveryModeCarriesTheReport() throws {
        guard
            case .daemonGetHealth(let health) = try Self.reply(
                "frame-response-get-health-recovery.json"),
            case .daemonGetHealth(let ready) = try Self.reply("frame-response-get-health.json")
        else {
            Issue.record("expected health replies")
            return
        }
        let recovery = try #require(health.recovery)
        #expect(recovery.reason == "schema_newer_than_app")
        #expect(recovery.appSchemaVersion == 2)
        #expect(recovery.dbSchemaVersion == 999)
        #expect(recovery.reportPath.hasSuffix(".json"))
        #expect(health.status == .degraded)
        #expect(ready.recovery == nil)
    }

    @Test func errorItemsDecodeWhateverTheirKind() throws {
        let listed = try Self.error("frame-error-with-items.json")
        #expect(listed.items?.count == 2)
        #expect(listed.items?.first?.kind == "issue")
        #expect(listed.items?.last?.fields?.first { $0.key == "path" }?.value == "metadata.name")
        #expect(listed.detail("omitted") == "1")

        let foreign = try Self.error("frame-error-unknown-item-kind.json")
        #expect(foreign.code == "secret.reference_unresolved")
        #expect(foreign.category == .failed)
        #expect(foreign.message == "2 references need a secret")
        #expect(foreign.items?.first?.kind == "hologram")
    }

    @Test func anErrorAboutTheManagerNamesTheResourceSelf() throws {
        let error = try Self.error("frame-error-manager-resource.json")
        #expect(error.resource == ResourceRef(kind: .manager, id: "self"))
        #expect(error.code == "resource.busy")
        #expect(try Self.error("frame-error-empty.json").items?.isEmpty == true)
    }
}

/// A fixture did not have the shape the test reads.
enum FixtureShapeError: Error {
    case unexpected(String)
}
