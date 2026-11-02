import Foundation
import StacklyClient
import Synchronization
import Testing

/// The config read methods of `StacklyClient` over a fake `send` (spec 0029 AC-1, AC-2,
/// AC-17): each builds the params of the contract and reads the matching reply, and a reply of
/// another kind is a protocol violation, not a result.
struct InstanceConfigClientTests {
    /// Records each call and answers with a fixed result.
    final class Fake: Sendable {
        private let calls = Mutex<[Call]>([])
        private let answer: Mutex<Result<Reply, ClientError>>

        init(_ answer: Result<Reply, ClientError>) {
            self.answer = Mutex(answer)
        }

        var made: [Call] { calls.withLock { $0 } }

        var client: InstanceConfigClient {
            InstanceConfigClient { [self] call, _ throws(ClientError) in
                calls.withLock { $0.append(call) }
                return try answer.withLock { $0 }.get()
            }
        }
    }

    private static func reply(_ file: String) throws -> Reply {
        try CreateContractTests.reply(file)
    }

    private static let instance = UUID(uuidString: "0192f3a5-0000-7000-8000-000000000001")!

    @Test func getSendsTheInstanceAndReadsTheView() async throws {
        let fake = Fake(.success(try Self.reply("frame-response-instance-config-get-fake.json")))
        let view = try await fake.client.get(instanceId: Self.instance)

        #expect(fake.made == [.instanceConfigGet(GetConfigParams(instanceId: Self.instance))])
        #expect(view.revision.number == 2)
        #expect(view.lastKnownGood == 2)
        #expect(view.drift == nil)
    }

    @Test func getReadsTheDriftOfAFileChangedOutsideStackly() async throws {
        let fake = Fake(.success(try Self.reply("frame-response-instance-config-get-drift.json")))
        let view = try await fake.client.get(instanceId: Self.instance)
        #expect(view.drift?.files.isEmpty == false)
    }

    @Test func getRefusesAReplyOfAnotherKind() async throws {
        let fake = Fake(
            .success(try Self.reply("frame-response-instance-config-validate-restart.json")))
        do {
            _ = try await fake.client.get(instanceId: Self.instance)
            Issue.record("expected a protocol violation")
        } catch {
            #expect(error == .rejected(ClientErrorCatalog.protocolViolation()))
        }
    }

    @Test func validateSendsTheBaseRevisionAndTheChange() async throws {
        let fake = Fake(
            .success(try Self.reply("frame-response-instance-config-validate-restart.json")))
        let change = ConfigChangeRequest.restore(1)
        let validation = try await fake.client.validate(
            instanceId: Self.instance, baseRevision: 2, change: change)

        #expect(
            fake.made == [
                .instanceConfigValidate(
                    ValidateConfigParams(
                        instanceId: Self.instance, baseRevision: 2, change: change))
            ])
        #expect(validation.valid)
        #expect(validation.impact == .restart)
        #expect(validation.changes.map(\.key) == ["workers"])
    }

    @Test func validateReadsTheFieldErrorsOfAnInvalidChange() async throws {
        let fake = Fake(
            .success(try Self.reply("frame-response-instance-config-validate-invalid.json")))
        let validation = try await fake.client.validate(
            instanceId: Self.instance, baseRevision: 2, change: .settings(JSONObject()))

        #expect(validation.valid == false)
        #expect(
            validation.fieldErrors.map { $0.code } == [
                "secret_read_only", "immutable_after_init", "out_of_range",
            ])
    }

    @Test func validateEndpointsAsksForADryRunAndReadsTheSuggestion() async throws {
        let fake = Fake(
            .success(try Self.reply("frame-response-instance-change-endpoints-dry-run.json")))
        let request = EndpointRequest(name: "postgres", port: .fixed(5433))
        let verdict = try await fake.client.validateEndpoints(
            instanceId: Self.instance, baseRevision: 2, endpoints: [request])

        #expect(
            fake.made == [
                .instanceChangeEndpoints(
                    ChangeEndpointsParams(
                        instanceId: Self.instance, baseRevision: 2, endpoints: [request],
                        dryRun: true))
            ])
        #expect(verdict.valid == false)
        #expect(verdict.problems.first?.suggestedPort == 5434)
    }

    @Test func validateEndpointsRefusesTheReplyOfARealChange() async throws {
        let fake = Fake(
            .success(try Self.reply("frame-response-instance-change-endpoints-applied.json")))
        do {
            _ = try await fake.client.validateEndpoints(
                instanceId: Self.instance, baseRevision: 2, endpoints: [])
            Issue.record("expected a protocol violation")
        } catch {
            #expect(error == .rejected(ClientErrorCatalog.protocolViolation()))
        }
    }

    @Test func aRevisionConflictPassesThroughWithTheActiveRevision() async throws {
        let body = try Fixture.ipc(ServerFrame.self, "frame-error-config-revision-conflict.json")
        guard case .error(let frame) = body else {
            Issue.record("not an error frame")
            return
        }
        let fake = Fake(.failure(.rejected(frame.error)))
        do {
            _ = try await fake.client.validate(
                instanceId: Self.instance, baseRevision: 1, change: .rollback)
            Issue.record("expected the refusal")
        } catch {
            #expect(error == .rejected(frame.error))
            #expect(frame.error.code == "config.revision_conflict")
        }
    }
}
