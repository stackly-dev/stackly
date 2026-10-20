import Foundation
import StacklyClient
import StacklyTestSupport
import Testing

/// `RevealedSecret` never shows its value in a text form (spec 0011 AC-18, AC-14).
struct RevealedSecretTests {
    private static func make(_ credential: FixtureCredential) -> RevealedSecret {
        RevealedSecret(reference: "local:erp/postgres/admin", value: credential.value)
    }

    @Test(
        .evidence(
            covers: ["AC-016", "REQ-016"],
            expected: "a revealed secret prints, dumps, and interpolates without its value"))
    func everyTextFormMasksTheValue() {
        let credential = FixtureCredential()
        let secret = Self.make(credential)

        var dumped = ""
        dump(secret, to: &dumped)
        var printed = ""
        print(secret, to: &printed)
        var debugPrinted = ""
        debugPrint(secret, to: &debugPrinted)

        let forms = [
            ("description", secret.description),
            ("debugDescription", secret.debugDescription),
            ("interpolation", "\(secret)"),
            ("reflecting", String(reflecting: secret)),
            ("describing", String(describing: secret)),
            ("dump", dumped), ("print", printed), ("debugPrint", debugPrinted),
            ("array", "\([secret])"),
        ]
        for (name, text) in forms {
            #expect(!text.contains(credential.value), "\(name) shows the value")
            credential.assertAbsent(in: text, what: "the \(name) of a revealed secret")
        }
    }

    @Test func descriptionNamesTheReferenceAndTheMask() {
        let secret = Self.make(FixtureCredential())
        #expect(secret.description == "RevealedSecret(local:erp/postgres/admin, ****)")
        #expect(secret.debugDescription == secret.description)
    }

    @Test func theMirrorHoldsTheReferenceAndTheMaskOnly() {
        let credential = FixtureCredential()
        let mirror = Mirror(reflecting: Self.make(credential))
        let children = Dictionary(
            uniqueKeysWithValues: mirror.children.compactMap { child in
                child.label.map { ($0, "\(child.value)") }
            })
        #expect(children == ["reference": "local:erp/postgres/admin", "value": "****"])
    }

    @Test func theValueStaysAvailableToTheCallerThatAsksForIt() {
        let credential = FixtureCredential()
        #expect(Self.make(credential).value == credential.value)
    }
}
