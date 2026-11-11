import Foundation
import StacklyClient
import Testing

/// `provider.validate` of protocol 3.6 from the shared fixtures (spec 0036,
/// *Kontrak wire*): the request, the reply frame, and the three reply
/// fixtures decode, and values a newer daemon sends read as
/// `.unrecognized`.
struct ProviderValidateContractTests {
    static func validation(_ file: String) throws -> ProviderValidation {
        try StacklyJSON.decoder().decode(
            ProviderValidation.self, from: Fixture.data("provider/\(file)"))
    }

    @Test func theRequestNamesTheAbsolutePath() throws {
        let request = try Fixture.ipc(Request.self, "request-provider-validate.json")
        guard case .providerValidate(let params) = request.call else {
            Issue.record("expected provider.validate")
            return
        }
        #expect(params.path == "/Users/dev/src/stackly/providers/mytool")
    }

    @Test func theReplyFrameCarriesTheUnsupportedVersionFixture() throws {
        guard
            case .response(let response) = try Fixture.ipc(
                ServerFrame.self, "frame-response-provider-validate.json"),
            case .providerValidate(let reply) = response.reply
        else {
            Issue.record("expected a provider.validate reply")
            return
        }
        #expect(reply == (try Self.validation("validation-unsupported-api-version.json")))
        #expect(reply.loadable == false)
        #expect(reply.identity == nil)
        let finding = try #require(reply.findings.first)
        #expect(finding.severity == .error)
        #expect(finding.origin == .package)
        #expect(finding.code == "unsupported_api_version")
        #expect(finding.location == "apiVersion")
    }

    @Test func aPublishablePackageHasItsIdentity() throws {
        let reply = try Self.validation("validation-publishable.json")
        #expect(reply.loadable && reply.publishable)
        #expect(reply.identity?.provider == "official/nats")
        #expect(reply.identity?.displayName == "NATS Server")
        #expect(reply.findings.isEmpty)
        #expect(reply.supportedApiVersions.map(\.status) == [.current])
    }

    @Test func anAuthoringFindingLoadsButIsNotPublishable() throws {
        let reply = try Self.validation("validation-authoring.json")
        #expect(reply.loadable)
        #expect(reply.publishable == false)
        #expect(reply.findings.map(\.origin) == [.authoring])
        #expect(reply.findings.first?.code == "token_not_allowed_here")
    }

    @Test func valuesANewerDaemonSendsReadUnrecognized() throws {
        let reply = try Self.validation("validation-unknown-values.json")
        #expect(reply.supportedApiVersions.first?.status == .unrecognized)
        #expect(reply.findings.first?.severity == .unrecognized)
        #expect(reply.findings.first?.origin == .unrecognized)
        #expect(reply.findings.first?.reason == nil)
        #expect(reply.identity == nil)
        #expect(reply.omitted == 2)
    }

    /// A daemon whose table marks a version `deprecated` sends that status
    /// and a `warning` finding (spec 0036 AC-1, AC-5, *Versi dan status*);
    /// both decode as named cases, not `.unrecognized`.
    @Test func aDeprecatedVersionAndItsWarningDecode() throws {
        var object = try #require(
            JSONSerialization.jsonObject(
                with: Fixture.data("provider/validation-publishable.json")) as? [String: Any])
        object["supportedApiVersions"] = [["name": "stackly/v1alpha1", "status": "deprecated"]]
        object["findings"] = [
            [
                "severity": "warning",
                "origin": "package",
                "file": "provider.yaml",
                "location": "apiVersion",
                "code": "deprecated_api_version",
                "message":
                    "apiVersion \"stackly/v1alpha1\" is deprecated; "
                    + "new packages use: stackly/v1beta1",
            ]
        ]
        object["findingCount"] = 1
        let data = try JSONSerialization.data(withJSONObject: object)

        let reply = try StacklyJSON.decoder().decode(ProviderValidation.self, from: data)

        #expect(reply.supportedApiVersions.map(\.status) == [.deprecated])
        #expect(reply.findings.map(\.severity) == [.warning])
        #expect(reply.findings.map(\.origin) == [.package])
        #expect(reply.loadable && reply.publishable)
    }
}
