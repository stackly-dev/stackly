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

    @ObservationIgnored private let call: (Call) async throws(ClientError) -> Reply
    @ObservationIgnored private let serverInstance: () -> UUID?
    @ObservationIgnored private var epoch = 0
    @ObservationIgnored private var inFlight: Set<Key> = []
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
        guard details[key] == nil, failures[key] == nil, inFlight.insert(key).inserted else {
            return
        }
        let started = epoch
        defer { inFlight.remove(key) }
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

    /// Drops every answer and failure: the store loaded a snapshot again.
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
