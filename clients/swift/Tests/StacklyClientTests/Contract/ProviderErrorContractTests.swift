import Foundation
import StacklyClient
import Testing

/// The `manifest` and `provider` codes of protocol 1.3 from the shared
/// fixtures and the catalog (spec 0009 AC-19): each issue is one `issue`
/// item with its file, location, code, and message, and a provider is named
/// by its revision key text.
struct ProviderErrorContractTests {
    static func error(_ file: String) throws -> ErrorBody {
        guard case .error(let frame) = try Fixture.ipc(ServerFrame.self, file) else {
            throw FixtureShapeError.unexpected(file)
        }
        return frame.error
    }

    @Test(arguments: ["frame-error-with-items.json", "frame-error-invalid-settings.json"])
    func everyIssueCarriesItsFileLocationCodeAndMessage(_ file: String) throws {
        let error = try Self.error(file)
        let items = try #require(error.items)
        #expect(error.detail("issueCount") == String(items.count))
        for item in items {
            #expect(item.kind == "issue")
            #expect(item.fields?.map(\.key) == ["file", "location", "code", "message"])
        }
    }

    @Test func aPackageErrorNamesThePackage() throws {
        let error = try Self.error("frame-error-with-items.json")
        #expect(error.code == "manifest.invalid")
        #expect(error.category == .invalidInput)
        #expect(error.detail("package") == "nats")
    }

    @Test func anUnsupportedCapabilityCarriesRuleBR016() throws {
        let error = try Self.error("frame-error-unsupported-capability.json")
        #expect(error.code == "provider.unsupported_capability")
        #expect(error.category == .unsupported)
        #expect(error.detail("capability") == "reload")
        #expect(error.rule == "BR-016")
    }

    @Test func aProviderIsNamedByItsRevisionKeyText() throws {
        let error = try Self.error("frame-error-provider-not-found.json")
        let resource = try #require(error.resource)
        #expect(resource.kind == .provider)
        let parts = resource.id.split(separator: "@", maxSplits: 1)
        #expect(parts.first == "official/nats")
        let revision = parts.last?.split(separator: "#", maxSplits: 1)
        #expect(revision?.first == "2.12.1-1")
        #expect(revision?.last?.count == 64)
    }

    @Test func theCatalogListsTheIssueItemsOfBothListCodes() throws {
        struct Items: Decodable {
            let kind: String
            let fields: [String]
            let max: Int
        }
        struct Code: Decodable {
            let code: String
            let retryable: Bool
            let items: Items?
        }
        struct Catalog: Decodable {
            let codes: [Code]
        }
        let catalog = try JSONDecoder().decode(
            Catalog.self, from: Fixture.repositoryFile("protocols/error-codes.json"))
        let codes = catalog.codes.filter {
            $0.code.hasPrefix("manifest.") || $0.code.hasPrefix("provider.")
        }
        #expect(codes.count == 7)
        #expect(codes.allSatisfy { $0.retryable == false })
        for code in ["manifest.invalid", "provider.invalid_settings"] {
            let items = try #require(codes.first { $0.code == code }?.items, "\(code)")
            #expect(items.kind == "issue")
            #expect(items.fields == ["file", "location", "code", "message"])
            #expect(items.max == 100)
        }
    }
}
