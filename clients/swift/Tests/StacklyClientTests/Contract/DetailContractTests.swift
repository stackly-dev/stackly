import Foundation
import StacklyClient
import Testing

/// The detail DTOs of protocol 1.14 from the shared fixtures (spec 0021
/// AC-17): the four detail triples decode with consistent connection
/// strings, the frames carry the same values, and foreign values of the open
/// enums read as `.unrecognized`.
struct DetailContractTests {
    /// One `instance/detail-*.json` file, the three values `stackly instance
    /// inspect --json` writes as its `result`.
    struct Triple: Decodable {
        let instance: InstanceSummary
        let detail: InstanceDetail
        let dataUsage: DataUsage?
    }

    static let names = ["postgresql", "nats", "unavailable", "package-unreadable"]

    static func triple(_ name: String) throws -> Triple {
        try StacklyJSON.decoder().decode(
            Triple.self, from: Fixture.data("instance/detail-\(name).json"))
    }

    static func reply(_ file: String) throws -> Reply {
        try CreateContractTests.reply(file)
    }

    /// Whether `state`, `reason`, `masked`, and `parts` agree as the DTO documents.
    static func isConsistent(_ connection: ConnectionString) -> Bool {
        switch connection.state {
        case .available:
            connection.masked != nil && !connection.parts.isEmpty && connection.reason == nil
        case .unavailable:
            connection.masked == nil && connection.parts.isEmpty && connection.reason != nil
        case .unrecognized:
            true
        }
    }

    @Test func eachTripleBelongsToOneInstanceAtOneRevision() throws {
        for name in Self.names {
            let triple = try Self.triple(name)
            #expect(triple.instance.id == triple.detail.instanceId)
            #expect(triple.instance.revision == triple.detail.revision)
            #expect(triple.instance.runtimeId == triple.detail.runtime.id)
        }
    }

    @Test func everyConnectionStringIsConsistent() throws {
        for name in Self.names {
            for connection in try Self.triple(name).detail.connections {
                #expect(Self.isConsistent(connection), "\(name): \(connection.name)")
            }
        }
    }

    @Test func onlyTheUnreadablePackageHasNoNameAndNoConnection() throws {
        for name in Self.names {
            let detail = try Self.triple(name).detail
            if name == "package-unreadable" {
                #expect(!detail.provider.packageReadable)
                #expect(detail.provider.displayName == nil)
                #expect(detail.connections.isEmpty)
                #expect(detail.secrets.allSatisfy { $0.title == $0.field })
            } else {
                #expect(detail.provider.packageReadable)
                #expect(detail.provider.displayName != nil)
            }
        }
    }

    @Test func theTriplesCoverAvailableUnavailableAndPortMayChange() throws {
        let postgresql = try Self.triple("postgresql")
        #expect(postgresql.detail.connections.map(\.state) == [.available, .available])
        #expect(postgresql.detail.connections.allSatisfy { !$0.portMayChange })
        #expect(postgresql.detail.secrets.first?.status == .present)
        #expect(postgresql.detail.dependents.map(\.ownership) == [.dedicated, .shared])
        #expect(postgresql.detail.dependents.map(\.leaseActive) == [true, false])
        #expect(postgresql.dataUsage?.complete == true)

        let nats = try Self.triple("nats")
        #expect(nats.detail.connections.allSatisfy { $0.reason == .portNotAssigned })
        #expect(nats.detail.connections.allSatisfy { $0.portMayChange })
        #expect(nats.detail.runtime.installMode == .external)
        #expect(nats.detail.runtime.artifactSha256 == nil)
        #expect(nats.dataUsage == nil)

        let retained = try Self.triple("unavailable")
        #expect(retained.detail.connections.allSatisfy { $0.reason == .retained })
        #expect(retained.dataUsage?.complete == false)
    }

    @Test func aSecretPartCarriesTheFieldAndTheReferenceButNeverAValue() throws {
        let uri = try #require(try Self.triple("postgresql").detail.connections.first)
        let secret = try #require(uri.parts.compactMap(\.secret).first)
        #expect(secret.field == "superuserPassword")
        #expect(secret.reference.hasPrefix("local:instance/"))
        #expect(uri.masked == "postgresql://postgres:****@127.0.0.1:5432/postgres")
    }

    @Test func theFramesCarryTheRequestsAndTheTripleValues() throws {
        let detailRequest = try Fixture.ipc(Request.self, "request-instance-get-detail.json")
        guard case .instanceGetDetail = detailRequest.call else {
            Issue.record("expected an instance.get_detail call")
            return
        }
        let usageRequest = try Fixture.ipc(Request.self, "request-instance-get-data-usage.json")
        guard case .instanceGetDataUsage(let params) = usageRequest.call else {
            Issue.record("expected an instance.get_data_usage call")
            return
        }
        #expect(params.cachedOnly == true)
        #expect(params.refresh != true)

        guard
            case .instanceGetDetail(let detail) = try Self.reply(
                "frame-response-instance-get-detail.json")
        else {
            Issue.record("expected an instance.get_detail reply")
            return
        }
        #expect(detail == (try Self.triple("postgresql")).detail)
        guard
            case .instanceGetDetail(let unreadable) = try Self.reply(
                "frame-response-instance-get-detail-package-unreadable.json")
        else {
            Issue.record("expected an instance.get_detail reply")
            return
        }
        #expect(unreadable == (try Self.triple("package-unreadable")).detail)

        func usage(_ file: String) throws -> DataUsage? {
            guard case .instanceGetDataUsage(let reply) = try Self.reply(file) else {
                Issue.record("expected an instance.get_data_usage reply")
                return nil
            }
            return reply.usage
        }
        let measured = try usage("frame-response-instance-get-data-usage.json")
        let incomplete = try usage("frame-response-instance-get-data-usage-incomplete.json")
        let empty = try usage("frame-response-instance-get-data-usage-empty.json")
        #expect(measured == (try Self.triple("postgresql")).dataUsage)
        #expect(incomplete == (try Self.triple("unavailable")).dataUsage)
        #expect(empty == nil)
    }

    @Test func foreignValuesOfTheDetailDecodeAsUnrecognized() throws {
        guard
            case .instanceGetDetail(let detail) = try Self.reply(
                "frame-response-instance-get-detail-unknown-values.json")
        else {
            Issue.record("expected an instance.get_detail reply")
            return
        }
        #expect(detail.startup == .unrecognized)
        #expect(detail.dependents[0].ownership == .unrecognized)
        #expect(detail.secrets[0].status == .unrecognized)
        #expect(detail.runtime.installMode == .unrecognized)
        #expect(detail.connections[0].state == .unrecognized)
        #expect(detail.connections[1].parts[1].kind == .unrecognized)
        #expect(detail.connections[2].reason == .unrecognized)
    }
}
