import Foundation
import Testing

@testable import StacklyClient

/// The client's error codes, categories, limits, and contract names against
/// `protocols/error-codes.json` and the shared fixtures (spec 0013 AC-4, AC-6,
/// AC-20).
struct ContractTests {
    struct CatalogCode: Decodable, Sendable {
        let code: String
        let category: String
        let retryable: Bool
        let details: [String]
    }

    struct Catalog: Decodable, Sendable {
        let codes: [CatalogCode]
    }

    static func catalog() throws -> [CatalogCode] {
        try JSONDecoder().decode(
            Catalog.self, from: Fixture.repositoryFile("protocols/error-codes.json")
        ).codes
    }

    /// The ten closed categories of spec 0006, in the catalog's words.
    static let categories = [
        "invalid_input", "not_found", "conflict", "unauthorized", "unsupported", "unavailable",
        "failed", "timeout", "cancelled", "internal",
    ]

    /// Every Swift category, through an exhaustive switch, so a new case fails
    /// to compile here.
    static func wireName(_ category: ErrorCategory) -> String {
        switch category {
        case .invalidInput, .notFound, .conflict, .unauthorized, .unsupported, .unavailable,
            .failed, .timeout, .cancelled, .internal:
            category.rawValue
        }
    }

    @Test func swiftKnowsExactlyTheTenClosedCategories() throws {
        let decoded = try Self.categories.map { name in
            try #require(ErrorCategory(rawValue: name), "Swift has no category \(name)")
        }
        #expect(Set(decoded.map(Self.wireName)) == Set(Self.categories))
        #expect(Set(try Self.catalog().map(\.category)).isSubset(of: Set(Self.categories)))
    }

    @Test func everyCatalogCodeDecodesWithACategorySwiftKnows() throws {
        for code in try Self.catalog() {
            let json =
                #"{"code":"\#(code.code)","category":"\#(code.category)","message":"m","retryable":\#(code.retryable)}"#
            let body = try StacklyJSON.decoder().decode(ErrorBody.self, from: Data(json.utf8))
            #expect(body.category.rawValue == code.category, "\(code.code)")
        }
    }

    @Test func clientCodesMatchTheCatalog() throws {
        let catalog = Dictionary(uniqueKeysWithValues: try Self.catalog().map { ($0.code, $0) })
        for entry in ClientErrorCatalog.entries {
            let expected = try #require(catalog[entry.code], "\(entry.code) is not in the catalog")
            #expect(entry.category.rawValue == expected.category, "\(entry.code)")
            #expect(entry.retryable == expected.retryable, "\(entry.code)")
            #expect(entry.details == expected.details, "\(entry.code)")
            #expect(entry.message.first?.isLowercase == true, "\(entry.code)")
            #expect(entry.message.hasSuffix(".") == false, "\(entry.code)")
        }
        #expect(catalog["ipc.not_connected"]?.retryable == true)
        #expect(catalog["ipc.call_timeout"]?.details == ["method", "seconds", "idempotencyKey"])
    }

    @Test func builtBodiesUseOnlyTheirCatalogKeys() {
        let bodies = [
            ClientErrorCatalog.endpointMissing(path: "/p"),
            ClientErrorCatalog.endpointUntrusted(path: "/p", reason: .symlink),
            ClientErrorCatalog.endpointUntrusted(path: nil, reason: .peerUidMismatch),
            ClientErrorCatalog.connectionRefused(path: "/p"),
            ClientErrorCatalog.ioError(op: "stat", errno: EACCES),
            ClientErrorCatalog.frame(.tooLarge(length: 9, max: 4)),
            ClientErrorCatalog.callTimeout(method: "daemon.get_info", limit: .seconds(10)),
            ClientErrorCatalog.callTimeout(
                method: "instance.start", limit: .milliseconds(10_500), idempotencyKey: "k-1"),
        ]
        let allowed = Dictionary(
            uniqueKeysWithValues: ClientErrorCatalog.entries.map { ($0.code, Set($0.details)) })
        for body in bodies {
            let keys = Set(body.details?.map(\.key) ?? [])
            #expect(keys.isSubset(of: allowed[body.code] ?? []), "\(body.code)")
        }
        #expect(
            ClientErrorCatalog.ioError(op: "stat", errno: EACCES).detail("kind")
                == "permission_denied")
        #expect(ClientErrorCatalog.frame(.tooLarge(length: 9, max: 4)).detail("length") == "9")
        let timeout = ClientErrorCatalog.callTimeout(
            method: "instance.start", limit: .milliseconds(10_500), idempotencyKey: "k-1")
        #expect(timeout.details?.map(\.key) == ["method", "seconds", "idempotencyKey"])
        #expect(timeout.detail("seconds") == "10")
        #expect(timeout.detail("idempotencyKey") == "k-1")
        let read = ClientErrorCatalog.callTimeout(method: "daemon.get_info", limit: .seconds(10))
        #expect(read.detail("idempotencyKey") == nil)
    }

    @Test func handshakeFixtureLimitsMatchTheSwiftConstants() throws {
        let frame = try Fixture.ipc(ServerFrame.self, "frame-response-handshake.json")
        guard case .response(let response) = frame,
            case .daemonHandshake(let accepted) = response.reply
        else {
            Issue.record("the fixture is not a handshake reply")
            return
        }
        #expect(accepted.limits.maxFrameBytes == ContractLimits.controlFrameMax)
        #expect(accepted.limits.maxJsonDepth == ContractLimits.maxJsonDepth)
    }

    @Test func contractNamesCoverEveryEventKindAndMethod() throws {
        #expect(
            ContractNames.eventKinds == [
                "events.gap", "daemon.shutting_down", "operation.state_changed",
                "operation.progress", "secret.changed", "instance.state_changed",
                "instance.health_changed", "instance.removed", "daemon.reconciling",
                "runtime.state_changed",
            ])
        let calls: [Call] = [
            .daemonGetInfo, .daemonGetHealth, .daemonGetCapabilities,
            .eventsUnsubscribe(UnsubscribeParams(subscriptionId: 1)),
        ]
        #expect(
            calls.map(\.methodName) == [
                "daemon.get_info", "daemon.get_health", "daemon.get_capabilities",
                "events.unsubscribe",
            ])
        #expect(Reply.eventsUnsubscribe.methodName == "events.unsubscribe")
    }

    @Test func protocolIsOnePointEleven() {
        #expect(ContractInfo.protocolVersion == ProtocolVersion(major: 1, minor: 14))
    }
}
