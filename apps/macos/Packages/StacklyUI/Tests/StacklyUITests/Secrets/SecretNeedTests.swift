import StacklyClient
import Testing

@testable import StacklyUI

/// The rows of the secret entry sheet, from a `secret.needs_secret` error or a blocker
/// (spec 0011 AC-11, *GUI*).
struct SecretNeedTests {
    @Test func theErrorFixtureGivesOneNeedPerReferenceInOrder() throws {
        let error = try ContractFixture.error("frame-error-secret-needs-secret")
        let needs = SecretNeed.needs(in: error)
        #expect(
            needs.map(\.reference) == [
                "local:erp/postgres/admin",
                "local:instance/0192f3a5-0000-7000-8000-000000000001/admin-password",
            ])
        #expect(needs.map(\.reason) == [.missing, .storeLocked])
        #expect(needs.map(\.generateAllowed) == [true, false])
        #expect(needs.map(\.id) == needs.map(\.reference))
    }

    @Test func anotherErrorCodeGivesNoNeeds() throws {
        let error = try ContractFixture.error("frame-error-secret-conflict-in-use")
        #expect(SecretNeed.needs(in: error).isEmpty)
    }

    @Test func itemsOfAnotherKindAndItemsWithoutAReferenceAreSkipped() {
        let error = SecretsScript.rejection(
            "secret.needs_secret", category: .unavailable,
            items: [
                ErrorItem(kind: "use", fields: [ErrorDetail(key: "reference", value: "local:a/b")]),
                ErrorItem(
                    kind: "reference", fields: [ErrorDetail(key: "reason", value: "missing")]),
                ErrorItem(
                    kind: "reference",
                    fields: [
                        ErrorDetail(key: "reference", value: "local:a/c"),
                        ErrorDetail(key: "reason", value: "a_reason_from_the_future"),
                        ErrorDetail(key: "generateAllowed", value: "maybe"),
                    ]),
            ])
        guard case .rejected(let body) = error else { return }
        let needs = SecretNeed.needs(in: body)
        #expect(needs.count == 1)
        #expect(needs.first?.reason == .unrecognized)
        #expect(needs.first?.generateAllowed == false)
    }

    @Test func aBlockerItemBecomesANeedWithTheSameFacts() {
        let item = NeedsSecretItem(
            reference: "local:a/b", reason: .accessDenied, generateAllowed: true)
        let need = SecretNeed(item: item, limits: SecretLimits(minLength: 12))
        #expect(need.reference == "local:a/b")
        #expect(need.reason == .accessDenied)
        #expect(need.generateAllowed)
        #expect(need.limits.minLength == 12)
    }

    @Test func everyReasonHasAnEnglishSentence() {
        let reasons: [NeedsSecretReason] = [
            .missing, .storeLocked, .accessDenied, .storeBusy, .storeUnsupported, .unrecognized,
        ]
        for reason in reasons {
            let need = SecretNeed(reference: "local:a/b", reason: reason, generateAllowed: false)
            #expect(need.reasonText.hasSuffix("."))
        }
    }
}
