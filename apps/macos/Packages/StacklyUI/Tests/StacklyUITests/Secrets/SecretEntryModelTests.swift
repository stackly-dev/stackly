import Foundation
import StacklyClient
import StacklyTestSupport
import Testing

@testable import StacklyUI

/// The rows of the secret entry sheet and their saves (spec 0011 AC-19, *GUI*).
@Suite(.timeLimit(.minutes(1)))
struct SecretEntryModelTests {
    private static let admin = "local:erp/postgres/admin"
    private static let other = "local:erp/postgres/replica"

    private static func needs() -> [SecretNeed] {
        [
            SecretNeed(
                reference: admin, reason: .missing, generateAllowed: true,
                limits: SecretLimits(minLength: 8)),
            SecretNeed(reference: other, reason: .storeLocked, generateAllowed: false),
        ]
    }

    private static func typed(_ text: String) -> SecretDraft {
        var draft = SecretDraft(allowsGenerate: false)
        draft.text = text
        return draft
    }

    private func setParams(_ call: Call?) -> SetSecretParams? {
        guard case .secretSet(let params) = call else { return nil }
        return params
    }

    @Test func aSaveSendsCreateWithTheDraftAndMarksTheRowSaved() async {
        let script = SecretsScript { call in
            guard case .secretSet(let params) = call else {
                return .failure(.notSent(ClientErrorCatalog.notConnected()))
            }
            return .success(SecretsScript.setReply(params.reference))
        }
        let model = SecretEntryModel(needs: Self.needs(), client: script.client)

        await model.save(reference: Self.admin, draft: SecretDraft(allowsGenerate: true))

        let params = setParams(script.made.first)
        #expect(params?.reference == Self.admin)
        #expect(params?.mode == .create)
        #expect(params?.input == .generate)
        #expect(params?.confirmStoredOnly == nil)
        #expect(model.rows[0].state == .saved(warnings: []))
        #expect(model.rows[1].state == .editing)
        #expect(!model.allSaved)
    }

    @Test func theMainButtonOpensOnceEveryRowIsSaved() async {
        let script = SecretsScript { call in
            guard case .secretSet(let params) = call else {
                return .failure(.notSent(ClientErrorCatalog.notConnected()))
            }
            return .success(SecretsScript.setReply(params.reference))
        }
        let model = SecretEntryModel(needs: Self.needs(), client: script.client)
        await model.save(reference: Self.admin, draft: SecretDraft(allowsGenerate: true))
        await model.save(reference: Self.other, draft: Self.typed("fixture-secret-0004"))
        #expect(model.allSaved)
    }

    @Test func aDraftThatDoesNotFitIsNeverSent() async {
        let script = SecretsScript { _ in .failure(.notSent(ClientErrorCatalog.notConnected())) }
        let model = SecretEntryModel(needs: Self.needs(), client: script.client)
        await model.save(reference: Self.admin, draft: Self.typed("short"))
        await model.save(reference: Self.other, draft: Self.typed(""))
        #expect(script.made.isEmpty)
        #expect(model.rows.allSatisfy { $0.state == .editing })
    }

    @Test func aShortValueWarningShowsOnTheSavedRow() async {
        let script = SecretsScript { call in
            guard case .secretSet(let params) = call else {
                return .failure(.notSent(ClientErrorCatalog.notConnected()))
            }
            return .success(
                SecretsScript.setReply(params.reference, warnings: [.shortValueNotScrubbed]))
        }
        let model = SecretEntryModel(needs: Self.needs(), client: script.client)
        await model.save(reference: Self.other, draft: Self.typed("tiny"))
        #expect(model.rows[1].state == .saved(warnings: [.shortValueNotScrubbed]))
    }

    @Test func aValueThatExistsOffersReplaceAndReplaceSendsTheReplaceMode() async {
        let script = SecretsScript { call in
            guard case .secretSet(let params) = call else {
                return .failure(.notSent(ClientErrorCatalog.notConnected()))
            }
            if params.mode == .create {
                return .failure(
                    SecretsScript.rejection(
                        "secret.conflict",
                        details: [
                            ErrorDetail(key: "reference", value: params.reference),
                            ErrorDetail(key: "reason", value: "already_exists"),
                        ]))
            }
            return .success(SecretsScript.setReply(params.reference))
        }
        let model = SecretEntryModel(needs: Self.needs(), client: script.client)
        let draft = Self.typed("fixture-secret-0005")

        await model.save(reference: Self.admin, draft: draft)
        #expect(model.rows[0].state == .exists)

        await model.replace(reference: Self.admin, draft: draft)
        #expect(setParams(script.made.last)?.mode == .replace)
        #expect(model.rows[0].state == .saved(warnings: []))
    }

    @Test func aConflictThatIsNotAnExistingValueIsAFailureNotAReplaceOffer() async {
        let script = SecretsScript { _ in
            .failure(
                SecretsScript.rejection(
                    "secret.conflict", message: "the secret reference is still used",
                    details: [ErrorDetail(key: "reason", value: "in_use")]))
        }
        let model = SecretEntryModel(needs: Self.needs(), client: script.client)
        await model.save(reference: Self.admin, draft: SecretDraft(allowsGenerate: true))
        #expect(model.rows[0].state == .failed("The secret reference is still used."))
    }

    @Test func aStoredOnlyReplaceAsksForConfirmationThenSendsTheFlag() async throws {
        let effects = try #require(
            try ContractFixture.error("frame-error-secret-confirmation-required").items)
        let confirmation = SecretsScript.rejection(
            "secret.confirmation_required", category: .invalidInput, items: effects)
        let script = SecretsScript { call in
            guard case .secretSet(let params) = call else {
                return .failure(.notSent(ClientErrorCatalog.notConnected()))
            }
            return params.confirmStoredOnly == true
                ? .success(SecretsScript.setReply(params.reference)) : .failure(confirmation)
        }
        let model = SecretEntryModel(needs: Self.needs(), client: script.client)
        let draft = Self.typed("fixture-secret-0006")

        await model.replace(reference: Self.admin, draft: draft)
        guard case .needsConfirmation(let listed) = model.rows[0].state else {
            Issue.record("expected the confirmation state")
            return
        }
        #expect(listed.first?.effect == .storedOnly)
        #expect(listed.first?.field == "adminPassword")
        #expect(model.pendingConfirmation?.reference == Self.admin)

        await model.confirmStoredOnly(reference: Self.admin, draft: draft)
        let last = setParams(script.made.last)
        #expect(last?.mode == .replace)
        #expect(last?.confirmStoredOnly == true)
        #expect(model.rows[0].state == .saved(warnings: []))
        #expect(model.pendingConfirmation == nil)
    }

    @Test func decliningTheConfirmationSendsNothingMore() async throws {
        let effects = try #require(
            try ContractFixture.error("frame-error-secret-confirmation-required").items)
        let script = SecretsScript { _ in
            .failure(
                SecretsScript.rejection(
                    "secret.confirmation_required", category: .invalidInput, items: effects))
        }
        let model = SecretEntryModel(needs: Self.needs(), client: script.client)
        await model.replace(reference: Self.admin, draft: Self.typed("fixture-secret-0007"))
        model.cancelConfirmation(reference: Self.admin)
        #expect(script.made.count == 1)
        #expect(model.rows[0].state == .exists)
        #expect(model.pendingConfirmation == nil)
    }

    @Test func aRejectionShowsTheDaemonsSafeSentenceAndHint() async {
        let script = SecretsScript { _ in
            .failure(
                .rejected(
                    ErrorBody(
                        code: "secret.store_locked", category: .unavailable,
                        message: "the keychain is locked", retryable: false, resource: nil,
                        fields: nil, hint: "unlock it and try again", details: nil,
                        operationId: nil, rule: nil, items: nil)))
        }
        let model = SecretEntryModel(needs: Self.needs(), client: script.client)
        await model.save(reference: Self.other, draft: Self.typed("fixture-secret-0008"))
        #expect(
            model.rows[1].state
                == .failed("The keychain is locked. Unlock it and try again."))
    }

    @Test func anUnknownOutcomeAsksTheUserToCheckBeforeTryingAgain() async {
        let script = SecretsScript { _ in
            .failure(
                .outcomeUnknown(
                    ClientErrorCatalog.callTimeout(
                        method: "secret.set", limit: .seconds(190), idempotencyKey: nil)))
        }
        let model = SecretEntryModel(needs: Self.needs(), client: script.client)
        await model.save(reference: Self.admin, draft: SecretDraft(allowsGenerate: true))
        #expect(
            model.rows[0].state
                == .failed("stacklyd didn’t answer in time. Check the status, then try again."))
    }

    @Test func nothingTheModelHoldsOrPrintsShowsTheTypedValue() async {
        let credential = FixtureCredential()
        let script = SecretsScript { call in
            guard case .secretSet(let params) = call else {
                return .failure(.notSent(ClientErrorCatalog.notConnected()))
            }
            return .success(SecretsScript.setReply(params.reference))
        }
        let model = SecretEntryModel(needs: Self.needs(), client: script.client)
        await model.save(reference: Self.admin, draft: Self.typed(credential.value))

        var dumped = ""
        dump(model, to: &dumped)
        credential.assertAbsent(in: dumped, what: "the dump of the entry model")
        credential.assertAbsent(in: "\(model.rows)", what: "the rows of the entry model")
    }
}
