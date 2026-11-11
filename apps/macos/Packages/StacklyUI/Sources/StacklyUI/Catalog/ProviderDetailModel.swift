import Foundation
import Observation
import StacklyClient

/// The detail of the selected provider (spec 0017, *Layar Catalog (GUI)*): it calls
/// `catalog.get_provider` when the selection changes and keeps each answer for the daemon
/// instance, the provider, and the catalog revision it came from.
/// The catalog does not change while a daemon runs, so an answer stays valid until the
/// store loads a snapshot again, which drops the cache. A provider that failed to load
/// shows its failure with Retry.
@Observable
public final class ProviderDetailModel {
    /// What the panel shows for one provider.
    public enum Phase: Equatable {
        /// The detail is on its way.
        case loading
        /// The detail loaded.
        case loaded(ProviderDetail)
        /// The detail could not load.
        case failed(FailureContent)
    }

    /// What an answer belongs to.
    struct Key: Hashable {
        let server: UUID?
        let provider: String
        let revision: UInt64
    }

    /// A load on its way: its key and the epoch it started in.
    private struct InFlight: Hashable {
        let key: Key
        let epoch: Int
    }

    @ObservationIgnored private let call: (Call) async throws(ClientError) -> Reply
    @ObservationIgnored private let serverInstance: () -> UUID?
    /// Counts the times the cache was dropped. The page keys its detail task on it, so a
    /// new snapshot (a restarted daemon, say) loads the selected provider again.
    private(set) var epoch = 0
    /// The loads on their way, each with the epoch it started in: a load of an older epoch
    /// never blocks one of the current epoch, whose answer is the only one kept.
    @ObservationIgnored private var inFlight: Set<InFlight> = []
    private var details: [Key: ProviderDetail] = [:]
    private var failures: [Key: FailureContent] = [:]

    /// Creates a model with an empty cache.
    /// - Parameters:
    ///   - call: Sends one call to the daemon.
    ///   - serverInstance: The ID of the daemon instance the link talks to now, or `nil`
    ///     when unknown. A restarted daemon has a new ID, so its answers are not reused.
    public init(
        call: @escaping (Call) async throws(ClientError) -> Reply,
        serverInstance: @escaping () -> UUID? = { nil }
    ) {
        self.call = call
        self.serverInstance = serverInstance
    }

    /// What the panel shows for `provider` at the catalog `revision`.
    public func phase(for provider: String, revision: UInt64) -> Phase {
        let key = key(provider, revision)
        if let detail = details[key] { return .loaded(detail) }
        if let failure = failures[key] { return .failed(failure) }
        return .loading
    }

    /// Loads the detail of `provider` unless it is cached, failed, or already on its way.
    /// A load that the caller cancels (the selection moved on) leaves no trace.
    public func load(_ provider: String, revision: UInt64) async {
        let key = key(provider, revision)
        let started = epoch
        let load = InFlight(key: key, epoch: started)
        guard details[key] == nil, failures[key] == nil, inFlight.insert(load).inserted else {
            return
        }
        defer { inFlight.remove(load) }
        do throws(ClientError) {
            let reply = try await call(.catalogGetProvider(GetProviderParams(provider: provider)))
            guard started == epoch, Task.isCancelled == false else { return }
            guard case .catalogGetProvider(let detail) = reply else {
                failures[key] = Self.failure(ClientErrorCatalog.protocolViolation())
                return
            }
            details[key] = detail
        } catch {
            guard started == epoch, Task.isCancelled == false else { return }
            failures[key] = Self.failure(error.body)
        }
    }

    /// Forgets the failure of `provider` and loads it again.
    public func retry(_ provider: String, revision: UInt64) async {
        failures[key(provider, revision)] = nil
        await load(provider, revision: revision)
    }

    /// Drops every answer and failure: the store loaded a snapshot again. A load still on its
    /// way keeps running, but its answer is dropped, and a new ``load(_:revision:)`` of the
    /// same provider starts at once instead of waiting for it.
    func invalidate() {
        epoch += 1
        details.removeAll()
        failures.removeAll()
    }

    private func key(_ provider: String, _ revision: UInt64) -> Key {
        Key(server: serverInstance(), provider: provider, revision: revision)
    }

    private static func failure(_ body: ErrorBody) -> FailureContent {
        FailureContent(title: "Couldn’t Load Provider", message: body.message, code: body.code)
    }
}
