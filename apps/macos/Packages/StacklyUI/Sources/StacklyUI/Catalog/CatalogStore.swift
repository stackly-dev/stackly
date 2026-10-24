import Foundation
import Observation
import StacklyClient

/// The provider catalog the daemon loaded (spec 0017, *Layar Catalog (GUI)*).
/// Its store loads `catalog.list_providers` in one page and keeps the daemon's display
/// order, since the daemon sorts the entries once and the GUI shows them as they come. It
/// also keeps the ``CatalogInfo`` of the last load. The catalog has no events and does not
/// change while a daemon runs, so the store only loads snapshots; a daemon that does not
/// know the method marks it unsupported.
@Observable
public final class CatalogStore {
    /// The loaded state of the last snapshot, observed by the views.
    @Observable final class Loaded {
        var info: CatalogInfo?
        var order: [String] = []
    }

    /// The store the shell keeps in sync.
    public let store: ResourceStore<ProviderSummary>
    /// The detail of the selected provider.
    public let detail: ProviderDetailModel
    @ObservationIgnored private let loaded: Loaded

    /// Creates an empty store.
    /// - Parameters:
    ///   - call: Sends one call to the daemon.
    ///   - serverInstance: The ID of the daemon instance now, for the detail cache.
    ///   - now: The wall clock.
    ///   - sleep: Waits; tests inject one that does not.
    public init(
        call: @escaping (Call) async throws(ClientError) -> Reply,
        serverInstance: @escaping () -> UUID? = { nil },
        now: @escaping () -> Date = { Date() },
        sleep: @escaping (Duration) async throws -> Void = { try await Task.sleep(for: $0) }
    ) {
        let loaded = Loaded()
        let detail = ProviderDetailModel(call: call, serverInstance: serverInstance)
        self.loaded = loaded
        self.detail = detail
        store = ResourceStore(
            kinds: ["catalog."],
            fetchPage: { _ throws(ClientError) in
                let reply = try await Self.list(call: call)
                loaded.info = reply.catalog
                loaded.order = (reply.items ?? []).map(\.provider)
                detail.invalidate()
                return Page(items: reply.items ?? [], next: nil)
            },
            fetchOne: { id throws(ClientError) in try await Self.provider(id, call: call) },
            revision: { $0.revision },
            reduce: { _ in .ignore },
            now: now,
            sleep: sleep)
    }

    /// The catalog as the daemon last reported it, or `nil` before the first snapshot.
    public var info: CatalogInfo? {
        loaded.info
    }

    /// The providers in the daemon's display order, or `nil` before the first snapshot.
    public var providers: [ProviderSummary]? {
        guard store.loadedAt != nil else { return nil }
        let held = store.items
        let ordered = loaded.order.compactMap { held[$0] }
        let known = Set(loaded.order)
        let extra = held.values.filter { known.contains($0.provider) == false }
            .sorted { $0.provider < $1.provider }
        return ordered + extra
    }

    /// The Catalog toolbar subtitle, or `nil` before the first snapshot.
    /// - Parameters:
    ///   - locale: The locale of the date.
    ///   - calendar: The calendar of the date.
    ///   - timeZone: The time zone of the date.
    public func subtitle(
        locale: Locale = .current, calendar: Calendar = .current, timeZone: TimeZone = .current
    ) -> String? {
        loaded.info.map {
            CatalogCopy.subtitle(info: $0, locale: locale, calendar: calendar, timeZone: timeZone)
        }
    }

    // MARK: - Calls

    /// `catalog.list_providers`.
    static func list(
        call: (Call) async throws(ClientError) -> Reply
    ) async throws(ClientError) -> ListProvidersReply {
        guard case .catalogListProviders(let reply) = try await call(.catalogListProviders) else {
            throw .rejected(ClientErrorCatalog.protocolViolation())
        }
        return reply
    }

    /// The summary of one provider from `catalog.get_provider`, or `nil` when the daemon
    /// answers `catalog.provider_not_found`.
    static func provider(
        _ id: String,
        call: (Call) async throws(ClientError) -> Reply
    ) async throws(ClientError) -> ProviderSummary? {
        let reply: Reply
        do throws(ClientError) {
            reply = try await call(.catalogGetProvider(GetProviderParams(provider: id)))
        } catch {
            if case .rejected(let body) = error, body.code == "catalog.provider_not_found" {
                return nil
            }
            throw error
        }
        guard case .catalogGetProvider(let detail) = reply else {
            throw .rejected(ClientErrorCatalog.protocolViolation())
        }
        return detail.summary
    }
}
