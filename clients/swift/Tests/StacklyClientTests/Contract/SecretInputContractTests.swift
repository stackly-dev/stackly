import Foundation
import StacklyClient
import Testing

/// `SecretInput` and `SecretText` from the shared fixtures (spec 0011
/// AC-5, AC-14): every source of a new value decodes to its own case, and a
/// value is a plain string on the wire.
struct SecretInputContractTests {
    @Test func everySourceDecodesToItsOwnCase() throws {
        #expect(
            try Fixture.ipc(SecretInput.self, "dto-secret-input-value.json")
                == .value("fixture-secret-0001"))
        #expect(try Fixture.ipc(SecretInput.self, "dto-secret-input-generate.json") == .generate)
        #expect(
            try Fixture.ipc(SecretInput.self, "dto-secret-input-copy-from.json")
                == .copyFrom("local:erp/postgres/admin"))
        guard
            case .bind(let reference) = try Fixture.ipc(
                SecretInput.self, "dto-secret-input-bind.json")
        else {
            Issue.record("expected a bind input")
            return
        }
        #expect(reference.hasPrefix("local:instance/"))
    }

    @Test func aValueEncodesAsAPlainStringUnderContent() throws {
        let input = SecretInput.value("fixture-secret-0002")
        let json = try JSONSerialization.jsonObject(with: StacklyJSON.encoder().encode(input))
        let object = try #require(json as? [String: String])
        #expect(object == ["type": "value", "content": "fixture-secret-0002"])
    }

    @Test func generateEncodesWithoutContent() throws {
        let json = try JSONSerialization.jsonObject(
            with: StacklyJSON.encoder().encode(SecretInput.generate))
        let object = try #require(json as? [String: Any])
        #expect(object["type"] as? String == "generate")
        #expect(object["content"] == nil)
    }
}
