import Foundation
import StacklyClient
import Synchronization
import Testing

/// The two detail methods of `StacklyClient` over a fake `send` (spec 0021 AC-4, AC-5): each
/// builds the params of the contract and reads the matching reply, and a reply of another kind
/// is a protocol violation, not a result.
struct InstanceDetailClientTests {
    /// Records each call and answers with a fixed result.
    final class Fake: Sendable {
        private let calls = Mutex<[Call]>([])
        private let answer: Mutex<Result<Reply, ClientError>>

        init(_ answer: Result<Reply, ClientError>) {
            self.answer = Mutex(answer)
        }

        var made: [Call] { calls.withLock { $0 } }

        var client: InstanceDetailClient {
            InstanceDetailClient { [self] call, _ throws(ClientError) in
                calls.withLock { $0.append(call) }
                return try answer.withLock { $0 }.get()
            }
        }
    }

    private static func reply(_ file: String) throws -> Reply {
        try DetailContractTests.reply(file)
    }

    @Test func getDetailSendsTheInstanceAndReadsTheDetail() async throws {
        let triple = try DetailContractTests.triple("postgresql")
        let fake = Fake(.success(.instanceGetDetail(triple.detail)))
        let detail = try await fake.client.getDetail(instanceId: triple.detail.instanceId)

        #expect(
            fake.made == [
                .instanceGetDetail(GetInstanceDetailParams(instanceId: triple.detail.instanceId))
            ])
        #expect(detail == triple.detail)
        #expect(detail.connections.count == 2)
    }

    @Test func getDetailRefusesADetailOfAnotherInstance() async throws {
        let triple = try DetailContractTests.triple("postgresql")
        let fake = Fake(.success(.instanceGetDetail(triple.detail)))
        do {
            _ = try await fake.client.getDetail(instanceId: UUID())
            Issue.record("expected a protocol violation")
        } catch {
            #expect(error == .rejected(ClientErrorCatalog.protocolViolation()))
        }
    }

    @Test func getDetailRefusesAReplyOfAnotherKind() async throws {
        let fake = Fake(.success(try Self.reply("frame-response-instance-get-data-usage.json")))
        do {
            _ = try await fake.client.getDetail(instanceId: UUID())
            Issue.record("expected a protocol violation")
        } catch {
            #expect(error == .rejected(ClientErrorCatalog.protocolViolation()))
        }
    }

    @Test func getDetailPassesTheDaemonsRefusalOn() async throws {
        let body = try Fixture.ipc(
            ServerFrame.self, "frame-error-instance-data-usage-unavailable-io.json")
        guard case .error(let frame) = body else {
            Issue.record("not an error frame")
            return
        }
        let fake = Fake(.failure(.rejected(frame.error)))
        do {
            _ = try await fake.client.getDetail(instanceId: UUID())
            Issue.record("expected the refusal")
        } catch {
            #expect(error == .rejected(frame.error))
        }
    }

    @Test func getDataUsageSendsTheFlagsAndReadsTheSize() async throws {
        let id = UUID()
        let fake = Fake(.success(try Self.reply("frame-response-instance-get-data-usage.json")))
        let usage = try await fake.client.getDataUsage(
            instanceId: id, refresh: true, cachedOnly: false)

        #expect(
            fake.made == [
                .instanceGetDataUsage(
                    GetDataUsageParams(instanceId: id, refresh: true, cachedOnly: false))
            ])
        #expect(usage?.complete == true)
    }

    @Test func getDataUsageReadsAnIncompleteSizeAndAnEmptyCache() async throws {
        let incomplete = Fake(
            .success(try Self.reply("frame-response-instance-get-data-usage-incomplete.json")))
        #expect(try await incomplete.client.getDataUsage(instanceId: UUID())?.complete == false)

        let empty = Fake(
            .success(try Self.reply("frame-response-instance-get-data-usage-empty.json")))
        let usage = try await empty.client.getDataUsage(instanceId: UUID(), cachedOnly: true)
        #expect(usage == nil)
    }

    @Test func getDataUsageRefusesAReplyOfAnotherKind() async throws {
        let triple = try DetailContractTests.triple("postgresql")
        let fake = Fake(.success(.instanceGetDetail(triple.detail)))
        do {
            _ = try await fake.client.getDataUsage(instanceId: triple.detail.instanceId)
            Issue.record("expected a protocol violation")
        } catch {
            #expect(error == .rejected(ClientErrorCatalog.protocolViolation()))
        }
    }
}
