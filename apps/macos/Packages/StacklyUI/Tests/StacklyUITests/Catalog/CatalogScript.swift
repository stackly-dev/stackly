import Foundation
import StacklyClient

@testable import StacklyUI

/// A daemon that answers the two catalog methods from fixtures, and counts the calls.
final class CatalogScript {
    var list: ListProvidersReply?
    var details: [String: ProviderDetail] = [:]
    var failure: ClientError?
    var getCalls: [String] = []
    var listCalls = 0

    func call(_ call: Call) async throws(ClientError) -> Reply {
        if let failure { throw failure }
        switch call {
        case .catalogListProviders:
            listCalls += 1
            guard let list else { throw .rejected(ClientErrorCatalog.protocolViolation()) }
            return .catalogListProviders(list)
        case .catalogGetProvider(let params):
            getCalls.append(params.provider)
            guard let detail = details[params.provider] else {
                throw .rejected(ClientErrorCatalog.protocolViolation())
            }
            return .catalogGetProvider(detail)
        default:
            throw .rejected(ClientErrorCatalog.protocolViolation())
        }
    }

    /// A script that serves the fixtures.
    static func fixture() throws -> CatalogScript {
        let script = CatalogScript()
        script.list = try CatalogFixture.list()
        script.details = try CatalogFixture.details()
        return script
    }

    /// A store reading through this script.
    @MainActor func store(
        serverInstance: @escaping () -> UUID? = { nil }
    ) -> CatalogStore {
        CatalogStore(
            call: { call throws(ClientError) in try await self.call(call) },
            serverInstance: serverInstance,
            now: { Date(timeIntervalSince1970: 100) },
            sleep: { _ in })
    }
}
