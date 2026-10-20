import Foundation
import StacklyClient
import StacklyTestSupport
import Synchronization
import Testing

/// The five secret methods of `StacklyClient` over a fake `send` (spec 0011 AC-18): each
/// builds the params of the contract, reads the matching reply, and treats a reply of
/// another kind as an unknown outcome.
struct SecretsClientTests {
    /// Records each call with its time limit and answers with a fixed result.
    final class Fake: Sendable {
        private let calls = Mutex<[(Call, Duration)]>([])
        private let answer: Mutex<Result<Reply, ClientError>>

        init(_ answer: Result<Reply, ClientError>) {
            self.answer = Mutex(answer)
        }

        var made: [(Call, Duration)] { calls.withLock { $0 } }

        var client: SecretsClient {
            SecretsClient { [self] call, options throws(ClientError) in
                calls.withLock { $0.append((call, options.timeout)) }
                return try answer.withLock { $0 }.get()
            }
        }
    }

    private static let admin = "local:erp/postgres/admin"

    private static func reply(_ file: String) throws -> Reply {
        try InstanceContractTests.reply(file)
    }

    @Test func listSecretsSendsTheFiltersAndReadsTheRowsAndTheStore() async throws {
        let fake = Fake(.success(try Self.reply("frame-response-secret-list.json")))
        let list = try await fake.client.listSecrets(ownerKind: .user, status: .present)

        let (call, timeout) = try #require(fake.made.first)
        #expect(
            call == .secretList(ListSecretsParams(ownerKind: .user, status: .present)))
        #expect(timeout == ContractLimits.callTimeout)
        #expect(list.items?.first?.reference == Self.admin)
        #expect(list.items?.count == 2)
        #expect(list.store.state == .available)
    }

    @Test func secretStatusSendsTheReferenceAndReadsTheStore() async throws {
        let fake = Fake(.success(try Self.reply("frame-response-secret-status.json")))
        let status = try await fake.client.secretStatus(reference: Self.admin)

        #expect(fake.made.first?.0 == .secretStatus(SecretStatusParams(reference: Self.admin)))
        #expect(status.summary.status == .missing)
        #expect(status.store.state == .locked)
    }

    @Test func setSecretSendsTheInputTheModeAndTheConfirmation() async throws {
        let fake = Fake(.success(try Self.reply("frame-response-secret-set.json")))
        let credential = FixtureCredential()
        let reply = try await fake.client.setSecret(
            reference: Self.admin, input: .value(credential.value), mode: .replace,
            expectedRevision: 3, confirmStoredOnly: true)

        let (call, timeout) = try #require(fake.made.first)
        #expect(
            call
                == .secretSet(
                    SetSecretParams(
                        reference: Self.admin, input: .value(credential.value), mode: .replace,
                        expectedRevision: 3, confirmStoredOnly: true)))
        #expect(timeout == SecretsClient.interactiveTimeout)
        #expect(reply.warnings == [.shortValueNotScrubbed])
        #expect(reply.effects?.first?.effect == .storedOnly)
        credential.assertAbsent(in: "\(reply)", what: "the printed reply of secret.set")
    }

    @Test func revealSecretReturnsAMaskedSecretWithTheDaemonsValue() async throws {
        let fake = Fake(.success(try Self.reply("frame-response-secret-reveal.json")))
        let revealed = try await fake.client.revealSecret(reference: Self.admin, purpose: .copy)

        let (call, timeout) = try #require(fake.made.first)
        #expect(call == .secretReveal(RevealSecretParams(reference: Self.admin, purpose: .copy)))
        #expect(timeout == SecretsClient.interactiveTimeout)
        #expect(revealed.reference == Self.admin)
        #expect(revealed.value == "not-a-real-secret")
        #expect(!"\(revealed)".contains("not-a-real-secret"))
    }

    @Test func revealSecretRefusesAReplyForAnotherReference() async throws {
        let fake = Fake(.success(try Self.reply("frame-response-secret-reveal.json")))
        do {
            _ = try await fake.client.revealSecret(
                reference: "local:erp/postgres/other", purpose: .show)
            Issue.record("expected a protocol violation")
        } catch {
            #expect(error == .outcomeUnknown(ClientErrorCatalog.protocolViolation()))
        }
    }

    @Test func deleteSecretSendsTheReferenceAndTheRevision() async throws {
        let fake = Fake(.success(try Self.reply("frame-response-secret-delete.json")))
        try await fake.client.deleteSecret(reference: Self.admin, expectedRevision: 3)

        let (call, timeout) = try #require(fake.made.first)
        #expect(
            call == .secretDelete(DeleteSecretParams(reference: Self.admin, expectedRevision: 3)))
        #expect(timeout == SecretsClient.interactiveTimeout)
    }

    @Test func aReplyOfAnotherKindIsAnUnknownOutcome() async throws {
        let wrong = Fake(.success(try Self.reply("frame-response-secret-delete.json")))
        do {
            _ = try await wrong.client.listSecrets()
            Issue.record("expected a protocol violation")
        } catch {
            #expect(error == .outcomeUnknown(ClientErrorCatalog.protocolViolation()))
        }
    }

    @Test func aDaemonErrorReachesTheCallerUnchanged() async throws {
        let body = try InstanceContractTests.error("frame-error-instance-name-taken.json")
        let fake = Fake(.failure(.rejected(body)))
        do {
            _ = try await fake.client.secretStatus(reference: Self.admin)
            Issue.record("expected the rejection")
        } catch {
            #expect(error == .rejected(body))
        }
    }

    @Test func foreignValuesOfTheOpenEnumsReadAsUnrecognized() throws {
        guard
            case .secretList(let list) = try Self.reply("frame-response-secret-unknown-values.json")
        else {
            Issue.record("expected a secret.list reply")
            return
        }
        let row = try #require(list.items?.first)
        #expect(row.ownerKind == .unrecognized)
        #expect(row.status == .unrecognized)
        #expect(row.valueOrigin == .unrecognized)
        #expect(list.store.backend == .unrecognized)
        #expect(list.store.state == .unrecognized)
        #expect(list.store.target == .unrecognized)
    }
}
