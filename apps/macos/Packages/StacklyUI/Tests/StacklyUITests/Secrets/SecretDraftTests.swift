import StacklyClient
import StacklyTestSupport
import Testing

@testable import StacklyUI

/// What the person chose and typed for a secret field (spec 0011 AC-19).
struct SecretDraftTests {
    private let limits = SecretLimits(minLength: 8, maxLength: 40, printableOnly: true)

    @Test func aFieldThatAllowsGenerateStartsOnGenerateAndOtherwiseOnEnter() {
        #expect(SecretDraft(allowsGenerate: true).source == .generate)
        #expect(SecretDraft(allowsGenerate: false).source == .enter)
    }

    @Test func generateAlwaysSubmitsAndSendsGenerate() {
        let draft = SecretDraft(allowsGenerate: true)
        #expect(draft.canSubmit(limits: limits))
        #expect(draft.input(limits: limits) == .generate)
    }

    @Test func enterSubmitsOnlyWhenTheTextFitsTheLimits() {
        var draft = SecretDraft(allowsGenerate: false)
        #expect(!draft.canSubmit(limits: limits))
        #expect(draft.input(limits: limits) == nil)
        draft.text = "short"
        #expect(!draft.canSubmit(limits: limits))
        let value = FixtureCredential().value.prefix(30)
        draft.text = String(value)
        #expect(draft.canSubmit(limits: limits))
        #expect(draft.input(limits: limits) == .value(String(value)))
        draft.text = String(value) + "\n"
        #expect(!draft.canSubmit(limits: limits))
    }

    @Test func copyFromSubmitsOnceAReferenceIsPicked() {
        var draft = SecretDraft(allowsGenerate: true)
        draft.source = .copyFrom
        #expect(!draft.canSubmit(limits: limits))
        draft.copyFromReference = "local:erp/postgres/admin"
        #expect(draft.input(limits: limits) == .copyFrom("local:erp/postgres/admin"))
    }

    @Test func clearForgetsTheTypedValueAndThePickedReference() {
        var draft = SecretDraft(allowsGenerate: false)
        draft.text = "fixture-secret-0003"
        draft.copyFromReference = "local:erp/postgres/admin"
        draft.clear()
        #expect(draft.text.isEmpty)
        #expect(draft.copyFromReference == nil)
    }

    @Test func noTextFormShowsTheTypedValue() {
        let credential = FixtureCredential()
        var draft = SecretDraft(allowsGenerate: false)
        draft.text = credential.value
        var dumped = ""
        dump(draft, to: &dumped)
        for (what, text) in [
            ("description", draft.description), ("debugDescription", draft.debugDescription),
            ("interpolation", "\(draft)"), ("dump", dumped),
        ] {
            credential.assertAbsent(in: text, what: "the \(what) of a draft")
        }
        #expect(draft.description == "SecretDraft(enter, ****)")
    }
}
