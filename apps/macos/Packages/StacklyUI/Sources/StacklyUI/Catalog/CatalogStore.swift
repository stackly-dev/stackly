import Foundation
import Observation
import StacklyClient

/// The provider catalog the daemon loaded (spec 0017, *Layar Catalog (GUI)*).
/// Its store loads `catalog.list_providers` in one page and keeps the daemon's display
/// order, since the daemon sorts the entries once and the GUI shows them as they come. It
/// also keeps the ``CatalogInfo`` and the catalog revocations of the last load. The catalog
/// changes while a daemon runs only when a provider is imported, approved, revoked, or
/// removed (spec 0037); the daemon then publishes `catalog.changed` and the store loads a
/// snapshot again, since the event carries no entry. A daemon that does not know the method
/// marks it unsupported.
@Observable
public final class CatalogStore {
    /// The loaded state of the last snapshot, observed by the views.
    @Observable final class Loaded {
        var info: CatalogInfo?
        var order: [String] = []
        var revocations: [CatalogRevocation] = []
    }

    /// The reply of the last `catalog.list_providers`, held until its generation completes,
    /// so the reply of a generation a newer one replaced changes nothing.
    final class PendingReply {
        var reply: ListProvidersReply?
    }

    /// The store the shell keeps in sync.
    public let store: ResourceStore<ProviderSummary>
    /// The detail of the selected provider.
    public let detail: ProviderDetailModel
    @ObservationIgnored private let loaded: Loaded
    @ObservationIgnored private let refresher: SnapshotCoalescer
    /// Sends one read to the daemon.
    @ObservationIgnored let call: (Call) async throws(ClientError) -> Reply
    /// Sends one mutation with its idempotency key through the mutation path.
    @ObservationIgnored let send: (Call, String) async -> MutationOutcome

    /// Creates an empty store.
    /// - Parameters:
    ///   - call: Sends one call to the daemon.
    ///   - send: Sends one mutation with its idempotency key, for the provider operations
    ///     of spec 0037; by default nothing is sent.
    ///   - serverInstance: The ID of the daemon instance now, for the detail cache.
    ///   - now: The wall clock.
    ///   - sleep: Waits; tests inject one that does not.
    public init(
        call: @escaping (Call) async throws(ClientError) -> Reply,
        send: @escaping (Call, String) async -> MutationOutcome = { _, _ in
            .notSent(ClientErrorCatalog.notConnected())
        },
        serverInstance: @escaping () -> UUID? = { nil },
        now: @escaping () -> Date = { Date() },
        sleep: @escaping (Duration) async throws -> Void = { try await Task.sleep(for: $0) }
    ) {
        let loaded = Loaded()
        let pending = PendingReply()
        let detail = ProviderDetailModel(call: call, serverInstance: serverInstance)
        let reload = ReloadBox()
        self.loaded = loaded
        self.detail = detail
        self.call = call
        self.send = send
        let store = ResourceStore<ProviderSummary>(
            kinds: ["catalog."],
            fetchPage: { _ throws(ClientError) in
                let reply = try await Self.list(call: call)
                pending.reply = reply
                return Page(items: reply.items ?? [], next: nil)
            },
            fetchOne: { id throws(ClientError) in try await Self.provider(id, call: call) },
            revision: { $0.revision },
            reduce: { event in
                if case .catalogChanged = event.kind {
                    reload.run?()
                }
                return .ignore
            },
            now: now,
            sleep: sleep)
        let refresher = SnapshotCoalescer { [weak store] in _ = await store?.resnapshot() }
        self.store = store
        self.refresher = refresher
        reload.run = { refresher.request() }
        // Only a snapshot that completed takes its reply and drops the detail cache: the
        // reply is the last one fetched, since a completed generation is the newest.
        store.didApply = { change in
            guard case .reloaded = change, let reply = pending.reply else { return }
            pending.reply = nil
            loaded.info = reply.catalog
            loaded.order = (reply.items ?? []).map(\.provider)
            loaded.revocations = reply.revocations ?? []
            detail.invalidate()
        }
    }

    /// The action `catalog.changed` runs, set once the resource store exists.
    private final class ReloadBox {
        var run: (@MainActor () -> Void)?
    }

    /// The revocations the installed catalog carries, from the last snapshot (spec 0037,
    /// AC-8). It is as old as the catalog itself.
    public var revocations: [CatalogRevocation] {
        loaded.revocations
    }

    /// The imported providers in display order, or `nil` before the first snapshot.
    public var localProviders: [ProviderSummary]? {
        providers?.filter { $0.trust == .local }
    }

    /// Loads the catalog again after a provider operation, without waiting for its event.
    /// It shares a snapshot with `catalog.changed`: a request while one runs asks for one more
    /// after it, so N requests make at most two `catalog.list_providers`.
    func reload() async {
        await refresher.run()
    }

    /// A model of the import flow over this store, for one window.
    /// - Parameters:
    ///   - picker: Asks the person for a folder.
    ///   - imported: Runs after an import went through, with the detail the daemon sent.
    func makeImportModel(
        picker: any FolderPicking = OpenPanelFolderPicker(),
        imported: @escaping @MainActor (ProviderDetail) async -> Void
    ) -> ProviderImportModel {
        ProviderImportModel(
            picker: picker, call: call, send: send,
            imported: { [weak self] detail in
                await self?.reload()
                await imported(detail)
            })
    }

    /// A model of approve, revoke, and remove over this store, for one window.
    func makeDecisionModel() -> ProviderDecisionModel {
        ProviderDecisionModel(
            send: send, refresh: { [weak self] in await self?.reload() })
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

/// Runs one snapshot at a time for requests that come together, such as `catalog.changed`
/// and the reload after an operation. A request while a snapshot runs asks for one more after
/// it, since the running one may have started before the change; N requests while one runs
/// make one more, not N.
@MainActor
final class SnapshotCoalescer {
    private var running: Task<Void, Never>?
    private var again = false
    private let snapshot: @MainActor () async -> Void

    /// Creates a coalescer around one snapshot.
    /// - Parameter snapshot: Loads one snapshot.
    init(snapshot: @escaping @MainActor () async -> Void) {
        self.snapshot = snapshot
    }

    /// Asks for a snapshot without waiting: starts one, or marks one more after the running
    /// one. It is synchronous, so a request made while a snapshot completes still joins it.
    func request() {
        guard running == nil else {
            again = true
            return
        }
        running = Task { @MainActor [weak self] in
            while let self {
                self.again = false
                await self.snapshot()
                guard self.again else {
                    self.running = nil
                    return
                }
            }
        }
    }

    /// Asks for a snapshot and waits until the snapshots that cover the request are done.
    func run() async {
        request()
        await running?.value
    }
}
