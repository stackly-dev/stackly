import Foundation
import Observation
import StacklyClient

/// The detail of the selected runtime (spec 0018, *Halaman Runtimes*): it calls
/// `runtime.get` when the selection changes and keeps each answer for the daemon instance,
/// the runtime, and the revision it came from.
///
/// A runtime that changed has a new revision, so it is read again; a snapshot drops the
/// cache. A runtime that failed to load shows its failure with Retry.
@Observable
public final class RuntimeDetailModel {
    /// What the inspector shows for one runtime.
    public enum Phase: Equatable {
        /// The detail is on its way.
        case loading
        /// The detail loaded.
        case loaded(RuntimeDetail)
        /// The detail could not load.
        case failed(FailureContent)
    }

    /// What an answer belongs to.
    struct Key: Hashable {
        let server: UUID?
        let runtime: UUID
        let revision: UInt64
    }

    @ObservationIgnored private let call: (Call) async throws(ClientError) -> Reply
    @ObservationIgnored private let serverInstance: () -> UUID?
    @ObservationIgnored private var epoch = 0
    @ObservationIgnored private var inFlight: Set<Key> = []
    private var details: [Key: RuntimeDetail] = [:]
    private var failures: [Key: FailureContent] = [:]

    /// Creates a model with an empty cache.
    ///
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

    /// What the inspector shows for `runtime` at `revision`.
    public func phase(for runtime: UUID, revision: UInt64) -> Phase {
        let key = key(runtime, revision)
        if let detail = details[key] { return .loaded(detail) }
        if let failure = failures[key] { return .failed(failure) }
        return .loading
    }

    /// Loads the detail of `runtime` unless it is cached, failed, or already on its way. A
    /// load that the caller cancels (the selection moved on) leaves no trace.
    public func load(_ runtime: UUID, revision: UInt64) async {
        let key = key(runtime, revision)
        guard details[key] == nil, failures[key] == nil, inFlight.insert(key).inserted else {
            return
        }
        let started = epoch
        defer { inFlight.remove(key) }
        do throws(ClientError) {
            let reply = try await call(.runtimeGet(GetRuntimeParams(runtimeId: runtime)))
            guard started == epoch, Task.isCancelled == false else { return }
            guard case .runtimeGet(let detail) = reply else {
                failures[key] = Self.failure(ClientErrorCatalog.protocolViolation())
                return
            }
            details[key] = detail
        } catch {
            guard started == epoch, Task.isCancelled == false else { return }
            failures[key] = Self.failure(error.body)
        }
    }

    /// Forgets the failure of `runtime` and loads it again.
    public func retry(_ runtime: UUID, revision: UInt64) async {
        failures[key(runtime, revision)] = nil
        await load(runtime, revision: revision)
    }

    /// Drops every answer and failure: the store loaded a snapshot again.
    func invalidate() {
        epoch += 1
        details.removeAll()
        failures.removeAll()
    }

    private func key(_ runtime: UUID, _ revision: UInt64) -> Key {
        Key(server: serverInstance(), runtime: runtime, revision: revision)
    }

    private static func failure(_ body: ErrorBody) -> FailureContent {
        FailureContent(title: "Couldn’t Load Runtime", message: body.message, code: body.code)
    }
}
