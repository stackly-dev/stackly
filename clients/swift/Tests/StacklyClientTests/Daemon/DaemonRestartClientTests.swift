import Foundation
import StacklyClient
import Synchronization
import Testing

/// The two restart methods of `StacklyClient` over a fake `send` (spec 0030 AC-16): the
/// preview reads without a key, the restart carries the confirmed digest and the key of the
/// action under the long limit, and a reply of another kind is a broken contract.
struct DaemonRestartClientTests {
    /// Records each call with its key and time limit and answers with a fixed result.
    final class Fake: Sendable {
        private let calls = Mutex<[(call: Call, key: String?, timeout: Duration)]>([])
        private let answer: Result<Reply, ClientError>

        init(_ answer: Result<Reply, ClientError>) {
            self.answer = answer
        }

        var made: [(call: Call, key: String?, timeout: Duration)] { calls.withLock { $0 } }

        var client: DaemonRestartClient {
            DaemonRestartClient { [self] call, key, options throws(ClientError) in
                calls.withLock { $0.append((call, key, options.timeout)) }
                return try answer.get()
            }
        }
    }

    private static let preview = RestartPreview(
        digest: "abc123", launchContext: .launchd, supervisorAvailable: true, instances: [],
        blockingOperations: [])

    @Test func previewSendsNoKeyAndReadsThePlan() async throws {
        let fake = Fake(.success(.daemonRestartPreview(Self.preview)))
        let preview = try await fake.client.preview()

        let made = try #require(fake.made.first)
        #expect(made.call == .daemonRestartPreview)
        #expect(made.key == nil)
        #expect(made.timeout == ContractLimits.callTimeout)
        #expect(preview == Self.preview)
    }

    @Test func restartSendsTheDigestTheForceFlagAndTheKeyUnderTheLongLimit() async throws {
        let accepted = ShutdownAccepted(
            serverInstanceId: UUID(), mode: .restart, planId: UUID(), exiting: true,
            stopAll: nil)
        let fake = Fake(.success(.daemonShutdown(accepted)))
        let reply = try await fake.client.restart(
            expectedPlanDigest: "abc123", force: true, idempotencyKey: "key-1")

        let made = try #require(fake.made.first)
        #expect(
            made.call
                == .daemonShutdown(
                    ShutdownParams(
                        mode: .restart, expectedPlanDigest: "abc123", force: true,
                        overrideLeases: nil)))
        #expect(made.key == "key-1")
        #expect(made.timeout == .seconds(190))
        #expect(made.timeout == SecretsClient.interactiveTimeout)
        #expect(made.timeout > ContractLimits.callTimeout)
        #expect(reply == accepted)
    }

    @Test func stopAllSendsTheExpectedIdsAndTheKeyUnderTheLongLimit() async throws {
        let first = UUID()
        let second = UUID()
        let result = StopAllResult(stopped: [first, second], failed: [], skipped: [])
        let fake = Fake(.success(.instanceStopAll(result)))

        let reply = try await fake.client.stopAll(
            expectedInstanceIds: [first, second], idempotencyKey: "key-2")

        let made = try #require(fake.made.first)
        #expect(
            made.call
                == .instanceStopAll(
                    StopAllParams(expectedInstanceIds: [first, second], overrideLeases: nil)))
        #expect(made.key == "key-2")
        #expect(made.timeout == SecretsClient.interactiveTimeout)
        #expect(reply == result)
    }

    @Test func theStopAllPreviewReadsWithoutAKey() async throws {
        let preview = StopAllPreview(
            items: [], alreadyStopping: [], skipped: [], atLoginCount: 0,
            affectedEnvironments: nil)
        let fake = Fake(.success(.instancePreviewStopAll(preview)))

        let reply = try await fake.client.previewStopAll()

        let made = try #require(fake.made.first)
        #expect(made.call == .instancePreviewStopAll(StopAllPreviewParams(overrideLeases: nil)))
        #expect(made.key == nil)
        #expect(reply == preview)
    }

    @Test func theOverrideOfALeaseReachesThePreviewAndTheStop() async throws {
        let preview = StopAllPreview(
            items: [], alreadyStopping: [], skipped: [], atLoginCount: 0,
            affectedEnvironments: [])
        let fake = Fake(.success(.instancePreviewStopAll(preview)))
        _ = try await fake.client.previewStopAll(overrideLeases: true)
        #expect(
            fake.made.first?.call
                == .instancePreviewStopAll(StopAllPreviewParams(overrideLeases: true)))

        let stops = Fake(.failure(.rejected(ClientErrorCatalog.protocolViolation())))
        _ = try? await stops.client.stopAll(
            expectedInstanceIds: [], idempotencyKey: "k", overrideLeases: true)
        #expect(
            stops.made.first?.call
                == .instanceStopAll(StopAllParams(expectedInstanceIds: [], overrideLeases: true)))

        let off = Fake(.failure(.rejected(ClientErrorCatalog.protocolViolation())))
        _ = try? await off.client.agentOff(idempotencyKey: "k", overrideLeases: true)
        #expect(
            off.made.first?.call
                == .daemonShutdown(
                    ShutdownParams(
                        mode: .agentOff, expectedPlanDigest: nil, force: false,
                        overrideLeases: true)))
    }

    @Test func aStopAllThePreviewNoLongerMatchesPassesTheRefusalThrough() async throws {
        let refusal = ClientErrorCatalog.protocolViolation()
        let fake = Fake(.failure(.rejected(refusal)))
        do {
            _ = try await fake.client.stopAll(expectedInstanceIds: [], idempotencyKey: "k")
            Issue.record("expected a refusal")
        } catch {
            #expect(error == .rejected(refusal))
        }
    }

    @Test func aReplyOfAnotherKindIsABrokenContract() async {
        let fake = Fake(.success(.daemonRestartPreview(Self.preview)))
        await #expect(throws: ClientError.self) {
            _ = try await fake.client.restart(expectedPlanDigest: "abc123", idempotencyKey: "k")
        }
    }

    @Test func aRefusalPassesThroughUnchanged() async throws {
        let refusal = ClientErrorCatalog.notConnected()
        let fake = Fake(.failure(.rejected(refusal)))
        do {
            _ = try await fake.client.preview()
            Issue.record("the preview did not throw")
        } catch {
            guard case .rejected(let body) = error else {
                Issue.record("expected a rejection, got \(error)")
                return
            }
            #expect(body == refusal)
        }
    }
}
