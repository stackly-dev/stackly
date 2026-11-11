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
}
