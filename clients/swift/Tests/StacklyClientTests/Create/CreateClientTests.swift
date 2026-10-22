import Foundation
import StacklyClient
import Synchronization
import Testing

/// The two create read methods of `StacklyClient` over a fake `send` (spec 0020 AC-1, AC-2,
/// AC-17): each builds the params of the contract, reads the matching reply, and treats a
/// reply of another kind as a broken contract.
struct CreateClientTests {
    /// Records each call with its time limit and answers with a fixed result.
    final class Fake: Sendable {
        private let calls = Mutex<[(Call, Duration)]>([])
        private let answer: Result<Reply, ClientError>

        init(_ answer: Result<Reply, ClientError>) {
            self.answer = answer
        }

        var made: [(Call, Duration)] { calls.withLock { $0 } }

        var client: CreateClient {
            CreateClient { [self] call, options throws(ClientError) in
                calls.withLock { $0.append((call, options.timeout)) }
                return try answer.get()
            }
        }
    }

    private static let runtime = UUID(uuidString: "0190a000-0000-7000-8000-000000000001")!

    @Test func getCreateFormSendsTheRuntimeAndTheProviderAndReadsTheForm() async throws {
        let reply = try CreateContractTests.reply("frame-response-instance-get-create-form.json")
        let fake = Fake(.success(reply))
        let form = try await fake.client.getCreateForm(
            runtimeId: Self.runtime, provider: "official/example")

        let (call, timeout) = try #require(fake.made.first)
        #expect(
            call
                == .instanceGetCreateForm(
                    GetCreateFormParams(runtimeId: Self.runtime, provider: "official/example")))
        #expect(timeout == ContractLimits.callTimeout)
        guard case .instanceGetCreateForm(let expected) = reply else {
            Issue.record("the fixture frame is not an instance.get_create_form reply")
            return
        }
        #expect(form == expected)
    }

    @Test func getCreateFormWithoutAProviderSendsNone() async throws {
        let reply = try CreateContractTests.reply("frame-response-instance-get-create-form.json")
        let fake = Fake(.success(reply))
        _ = try await fake.client.getCreateForm(runtimeId: Self.runtime)

        #expect(
            fake.made.first?.0
                == .instanceGetCreateForm(
                    GetCreateFormParams(runtimeId: Self.runtime, provider: nil)))
    }

    @Test func preflightSendsTheDraftAndReadsTheChecks() async throws {
        let reply = try CreateContractTests.reply(
            "frame-response-instance-preflight-port-conflict.json")
        let fake = Fake(.success(reply))
        let draft = PreflightCreateParams(
            name: "orders-db", displayName: nil,
            provider: ProviderSelector(name: "official/example", revision: nil),
            runtimeId: Self.runtime, endpoints: nil, settings: JSONObject(), secrets: nil,
            start: nil)
        let preflight = try await fake.client.preflight(draft)

        #expect(fake.made.first?.0 == .instancePreflight(draft))
        guard case .instancePreflight(let expected) = reply else {
            Issue.record("the fixture frame is not an instance.preflight reply")
            return
        }
        #expect(preflight == expected)
    }

    @Test func aReplyOfAnotherKindIsABrokenContract() async throws {
        let reply = try CreateContractTests.reply(
            "frame-response-instance-preflight-port-conflict.json")
        let fake = Fake(.success(reply))
        await #expect(throws: ClientError.self) {
            _ = try await fake.client.getCreateForm(runtimeId: Self.runtime)
        }
    }

    @Test func aDaemonErrorPassesThrough() async throws {
        let error = ClientError.rejected(
            ErrorBody(
                code: "instance.runtime_not_found", category: .notFound,
                message: "no runtime with that id", retryable: false, resource: nil,
                fields: nil, hint: nil, details: nil, operationId: nil, rule: nil, items: nil))
        let fake = Fake(.failure(error))
        do {
            _ = try await fake.client.getCreateForm(runtimeId: Self.runtime)
            Issue.record("expected the daemon error")
        } catch {
            #expect(error.body.code == "instance.runtime_not_found")
        }
    }
}
