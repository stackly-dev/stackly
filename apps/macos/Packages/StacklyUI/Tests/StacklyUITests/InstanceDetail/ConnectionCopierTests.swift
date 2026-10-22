import AppKit
import Foundation
import StacklyClient
import StacklyTestSupport
import Testing

@testable import StacklyUI

/// Copy Connection String of the More menu and of the command (spec 0021 AC-12, AC-13,
/// AC-18): nothing is revealed or written until the action runs, a string with a secret goes
/// through `secret.reveal { copy }` and the concealed pasteboard, and a value never reaches
/// the feedback.
@MainActor
@Suite(.timeLimit(.minutes(1)))
struct ConnectionCopierTests {
    private let postgres: DetailFixture.Triple
    private let nats: DetailFixture.Triple
    private let pasteboard = FakePasteboard()
    private let sleeper = ManualSleep()

    init() throws {
        postgres = try DetailFixture.triple("postgresql")
        nats = try DetailFixture.triple("nats")
    }

    private var concealed: ConcealedPasteboard {
        let sleeper = sleeper
        return ConcealedPasteboard(
            pasteboard: pasteboard, sleep: { try await sleeper.sleep(for: $0) })
    }

    private func secrets(value: String?) -> (client: SecretsClient, script: SecretsScript) {
        let script = SecretsScript { call in
            guard case .secretReveal(let params) = call, let value else {
                return .failure(
                    SecretsScript.rejection(
                        "secret.store_locked", category: .unavailable,
                        message: "the keychain is locked"))
            }
            return .success(SecretsScript.revealReply(params.reference, value))
        }
        return (script.client, script)
    }

    @Test func aStringWithASecretIsRevealedForCopyJoinedAndWrittenConcealed() async throws {
        let credential = FixtureCredential()
        let (client, script) = secrets(value: credential.value)
        let connection = try #require(postgres.detail.connections.first)

        let feedback = await ConnectionCopier.copy(
            connection, secrets: client, pasteboard: concealed)

        let reference = try #require(postgres.detail.secrets.first?.reference)
        #expect(
            script.made == [.secretReveal(RevealSecretParams(reference: reference, purpose: .copy))]
        )
        #expect(feedback.tone == .positive)
        #expect(feedback.message == "Connection string copied.")
        #expect(
            pasteboard.holds("postgresql://postgres:\(credential.value)@127.0.0.1:5432/postgres"))
        credential.assertAbsent(in: "\(feedback)", what: "the feedback")
        credential.assertAbsent(in: feedback.message + (feedback.detail ?? ""), what: "its words")
    }

    @Test func aStringWithoutASecretGoesToThePlainPasteboardAndRevealsNothing() async throws {
        let (client, script) = secrets(value: nil)
        let connection = try #require(nats.detail.connections.first)
        var written: [String] = []

        let feedback = await ConnectionCopier.copy(
            connection, secrets: client, pasteboard: concealed, write: { written.append($0) })

        // The NATS fixture entry is unavailable; build an available one without a secret.
        #expect(feedback.tone == .critical)
        #expect(written.isEmpty)
        #expect(script.made.isEmpty)

        let plain = ConnectionString(
            name: "uri", title: "Client URL", state: .available, reason: nil,
            masked: "nats://127.0.0.1:4222",
            parts: [ConnectionPart(kind: .text, text: "nats://127.0.0.1:4222", secret: nil)],
            portMayChange: false)
        let ok = await ConnectionCopier.copy(
            plain, secrets: client, pasteboard: concealed, write: { written.append($0) })
        #expect(ok.tone == .positive)
        #expect(ok.message == "Client URL copied.")
        #expect(ok.detail == nil)
        #expect(written == ["nats://127.0.0.1:4222"])
        #expect(script.made.isEmpty)
        #expect(pasteboard.changeCount == 0)
    }

    @Test func aFailedRevealCopiesNothingAndSaysWhy() async throws {
        let (client, _) = secrets(value: nil)
        let connection = try #require(postgres.detail.connections.first)

        let feedback = await ConnectionCopier.copy(
            connection, secrets: client, pasteboard: concealed)

        #expect(feedback.tone == .critical)
        #expect(feedback.message == "Couldn’t copy Connection string.")
        #expect(feedback.detail?.contains("keychain") == true)
        #expect(pasteboard.changeCount == 0)
    }

    // MARK: - The action

    @Test func theActionIsOffWithTheReasonWhenThereIsNothingToCopy() throws {
        let (client, _) = secrets(value: nil)
        let unreadable = try DetailFixture.triple("package-unreadable").detail
        let waiting = try DetailFixture.triple("nats").detail

        let none = ConnectionCopier.action(detail: nil, secrets: client, report: { _ in })
        let unread = ConnectionCopier.action(detail: unreadable, secrets: client, report: { _ in })
        let noPort = ConnectionCopier.action(detail: waiting, secrets: client, report: { _ in })
        let ready = ConnectionCopier.action(detail: postgres.detail, secrets: client) { _ in }

        #expect(none.disabledReason == "No connection string available.")
        #expect(unread.disabledReason == DetailFormat.packageUnavailableText)
        #expect(noPort.disabledReason == "Start the instance to assign its port.")
        #expect(ready.disabledReason == nil)
        #expect(ready.title == "Copy Connection String")
        #expect(ready.id == "copyConnectionString")
        #expect(ready.mutates == false)
    }

    @Test func buildingTheActionRevealsAndWritesNothing() throws {
        let (client, script) = secrets(value: FixtureCredential().value)
        _ = ConnectionCopier.action(detail: postgres.detail, secrets: client, report: { _ in })

        #expect(script.made.isEmpty)
        #expect(pasteboard.changeCount == 0)
    }
}
