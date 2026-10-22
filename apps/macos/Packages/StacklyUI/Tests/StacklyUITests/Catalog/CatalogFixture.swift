import Foundation
import StacklyClient
import Testing

@testable import StacklyUI

/// The catalog contract fixtures in `protocols/fixtures/catalog/`, the same files the Rust
/// golden tests of `stackly provider` read, decoded with ``StacklyJSON``.
enum CatalogFixture {
    /// The `ListProvidersReply` fixture.
    static func list() throws -> ListProvidersReply {
        try read("list-providers")
    }

    /// The `ProviderDetail` fixture `provider-<name>`.
    static func detail(_ name: String) throws -> ProviderDetail {
        try read("provider-\(name)")
    }

    /// The fixture providers by ID.
    static func details() throws -> [String: ProviderDetail] {
        var all: [String: ProviderDetail] = [:]
        for name in ["nats", "intelonly", "blockedhost", "broken", "zenith", "postgresql"] {
            let detail = try detail(name)
            all[detail.summary.provider] = detail
        }
        return all
    }

    /// The list reply of the `unknown-values` IPC fixture.
    static func unknownList() throws -> ListProvidersReply {
        let reply = try ContractFixture.reply(
            "frame-response-catalog-list-providers-unknown-values")
        guard case .catalogListProviders(let list) = reply else {
            throw ContractFixture.Mismatch(description: "not a catalog.list_providers reply")
        }
        return list
    }

    /// The detail of the `unknown-values` IPC fixture.
    static func unknownDetail() throws -> ProviderDetail {
        let reply = try ContractFixture.reply("frame-response-catalog-get-provider-unknown-values")
        guard case .catalogGetProvider(let detail) = reply else {
            throw ContractFixture.Mismatch(description: "not a catalog.get_provider reply")
        }
        return detail
    }

    private static func read<T: Decodable>(_ name: String) throws -> T {
        let url = SourceTree.url(of: "protocols/fixtures/catalog/\(name).json")
        return try StacklyJSON.decoder().decode(T.self, from: Data(contentsOf: url))
    }
}
