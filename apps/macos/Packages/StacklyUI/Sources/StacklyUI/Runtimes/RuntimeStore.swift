import Foundation
import Observation
import StacklyClient

/// How an install the person started from the Catalog ended, for the window to report even
/// after the person left the Catalog (spec 0018, *Kolom aksi Catalog*).
public struct FinishedInstall: Equatable, Sendable {
    /// How the install ended.
    public enum Outcome: Equatable, Sendable {
        /// The runtime is installed and verified.
        case succeeded
        /// The install was cancelled and kept nothing.
        case cancelled
        /// The install failed; the Catalog detail panel shows why.
        case failed
    }

    /// The operation.
    public let operationId: UUID
    /// The name shown for the software, such as "PostgreSQL".
    public let name: String
    /// The exact version.
    public let version: String
    /// How it ended.
    public let outcome: Outcome

    /// Creates a record.
    public init(operationId: UUID, name: String, version: String, outcome: Outcome) {
        self.operationId = operationId
        self.name = name
        self.version = version
        self.outcome = outcome
    }
}

extension FinishedInstall {
    /// What the window says when the install ended, or `nil` for a failure, which the
    /// Catalog detail panel shows instead (spec 0018, *Kolom aksi Catalog*).
    public var feedback: Feedback? {
        switch outcome {
        case .succeeded: .success("\(name) \(version) installed and verified.")
        case .cancelled: .neutral("Install of \(name) \(version) was cancelled. Nothing was kept.")
        case .failed: nil
        }
    }
}

/// Why an install did not start or did not finish, kept per provider for the Catalog detail
/// panel (spec 0018, *Kolom aksi Catalog*): the daemon's words, never a card.
public struct RuntimeInstallFailure: Equatable, Sendable {
    /// The provider ID, such as `official/postgresql`.
    public let provider: String
    /// The message, hint, and code of the daemon.
    public let message: String
    /// What the daemon suggests, or `nil`.
    public let hint: String?
    /// The error code.
    public let code: String

    /// Creates a failure from a daemon error.
    public init(provider: String, error: ErrorBody) {
        self.provider = provider
        message = DaemonText.sentence(error.message)
        hint = error.hint.map(DaemonText.sentence)
        code = error.code
    }
}

/// The managed runtimes the daemon holds, kept from snapshots plus `runtime.state_changed`
/// events, and the installs that have not ended (spec 0018, *Halaman Runtimes*).
///
/// Its store loads `runtime.list` and refetches one runtime with `runtime.get` after each
/// event, since the event carries no payload. The first page of a snapshot also names the
/// installs that are running; ``installing`` keeps them, and follows the operation center
/// so an install started elsewhere (the CLI) shows up too. The Get action of the Catalog
/// lives here, so a click survives a change of page: ``recordStarted(operationId:provider:name:version:)``
/// follows the operation to its end, and ``finishedInstalls`` tells the window how it ended.
@Observable
public final class RuntimeStore {
    /// The most runtimes one `runtime.list` page holds.
    static var pageLimit: UInt64 { 200 }
    /// How long a key whose outcome is unknown can be reused, in seconds.
    static var keyLifetime: TimeInterval { 300 }

    /// What the views read besides the store.
    @Observable final class Held {
        var installing: [InstallingRuntime] = []
        var failures: [String: RuntimeInstallFailure] = [:]
        /// The artifacts whose Get was clicked and has no reply yet, by digest.
        var starting: Set<String> = []
    }

    /// An install the person started, followed to its end.
    private struct Recorded {
        let provider: String?
        var name: String?
        var version: String?
    }

    /// A key kept after an unknown outcome.
    private struct StoredKey {
        let key: String
        let at: Date
    }

    /// The store the shell keeps in sync.
    public let store: ResourceStore<RuntimeSummary>
    /// The detail of the selected runtime.
    public let detail: RuntimeDetailModel
    /// The installs that ended after ``recordStarted(operationId:provider:name:version:)``,
    /// in order. One window consumes it.
    public let finishedInstalls: AsyncStream<FinishedInstall>

    @ObservationIgnored private let held: Held
    @ObservationIgnored private let call: (Call) async throws(ClientError) -> Reply
    @ObservationIgnored let send: @MainActor (Call, String) async -> MutationOutcome
    @ObservationIgnored private let updates: @MainActor (UUID) -> AsyncStream<OperationSnapshot>
    @ObservationIgnored private let now: () -> Date
    @ObservationIgnored private let continuation: AsyncStream<FinishedInstall>.Continuation
    @ObservationIgnored private var recorded: [UUID: Recorded] = [:]
    @ObservationIgnored private var watchers: [UUID: Task<Void, Never>] = [:]
    @ObservationIgnored private var keys: [String: StoredKey] = [:]
    @ObservationIgnored private var follower: Task<Void, Never>?

    /// Creates an empty store.
    ///
    /// - Parameters:
    ///   - call: Sends one read to the daemon.
    ///   - send: Sends one mutation with its idempotency key.
    ///   - updates: The updates of an operation, ending after its final state.
    ///   - serverInstance: The ID of the daemon instance now, for the detail cache.
    ///   - now: The wall clock.
    ///   - sleep: Waits; tests inject one that does not.
    public init(
        call: @escaping (Call) async throws(ClientError) -> Reply,
        send: @escaping @MainActor (Call, String) async -> MutationOutcome = { _, _ in
            .notSent(ClientErrorCatalog.notConnected())
        },
        updates: @escaping @MainActor (UUID) -> AsyncStream<OperationSnapshot> = { _ in
            AsyncStream { $0.finish() }
        },
        serverInstance: @escaping () -> UUID? = { nil },
        now: @escaping () -> Date = { Date() },
        sleep: @escaping (Duration) async throws -> Void = { try await Task.sleep(for: $0) }
    ) {
        let held = Held()
        self.call = call
        self.send = send
        self.updates = updates
        self.now = now
        (finishedInstalls, continuation) = AsyncStream.makeStream(of: FinishedInstall.self)
        let detail = RuntimeDetailModel(call: call, serverInstance: serverInstance)
        self.detail = detail
        store = ResourceStore(
            kinds: ["runtime."],
            fetchPage: { token throws(ClientError) in
                let reply = try await Self.page(after: token, call: call)
                if token == nil {
                    held.installing = reply.installing ?? []
                    detail.invalidate()
                }
                return Page(items: reply.items ?? [], next: reply.next.map(Self.pageToken))
            },
            fetchOne: { id throws(ClientError) in try await Self.runtime(id, call: call) },
            revision: { $0.revision },
            reduce: Self.reduce,
            isGone: { $0.state == .absent },
            now: now,
            sleep: sleep)
        self.held = held
    }

    // MARK: - What the views show

    /// The runtimes by name, then newest version first, then ID.
    public var runtimes: [RuntimeSummary] {
        store.items.values.sorted(by: Self.order)
    }

    /// The number of runtimes, or `nil` before the first snapshot loaded.
    public var loadedCount: Int? {
        store.loadedAt == nil ? nil : store.count
    }

    /// The Runtimes page subtitle ("1 installed", "3 installed"), or `nil` before the first
    /// snapshot loaded.
    public var subtitle: String? {
        loadedCount.map { "\($0) installed" }
    }

    /// The installs that have not ended, in the order the daemon named them.
    public var installing: [InstallingRuntime] {
        held.installing
    }

    /// How many installed runtimes run `software`, the number of "{n} installed" on a
    /// Catalog card.
    ///
    /// - Parameter software: `ProviderSummary.software`; `nil` counts nothing.
    public func installedCount(software: String?) -> Int {
        guard let software else { return 0 }
        return store.items.values.count { $0.software == software && $0.state == .installed }
    }

    /// The installed runtime built from the artifact with `sha256`, or `nil`.
    public func installedRuntime(sha256: String) -> RuntimeSummary? {
        store.items.values.first { $0.state == .installed && $0.artifactSha256 == sha256 }
    }

    /// The install that has not ended for the artifact with `sha256`, or `nil`.
    public func installingRuntime(sha256: String) -> InstallingRuntime? {
        held.installing.first { $0.sha256 == sha256 }
    }

    /// Whether Get was clicked for the artifact with `sha256` and the daemon has not
    /// answered yet.
    public func isStarting(sha256: String) -> Bool {
        held.starting.contains(sha256)
    }

    /// Why the last install of `provider` did not start or did not finish, or `nil`.
    public func failure(for provider: String) -> RuntimeInstallFailure? {
        held.failures[provider]
    }

    /// Forgets the failure of `provider`: the person dismissed it or tried again.
    public func dismissFailure(for provider: String) {
        held.failures[provider] = nil
    }

    // MARK: - Following operations

    /// Follows the operation center: an `install_runtime` operation that starts or ends,
    /// wherever it was started, reads `runtime.list` again so ``installing`` stays true.
    ///
    /// - Parameter operations: The operation center of the shell.
    public func follow(_ operations: OperationCenter) {
        follower?.cancel()
        follower = Task { [weak self] in
            var known: Set<UUID> = []
            let ids = Observations {
                Set(operations.active.filter { $0.kind == .installRuntime }.map(\.id))
            }
            for await current in ids {
                guard let self else { return }
                if current != known {
                    known = current
                    await self.reloadInstalling()
                }
            }
        }
    }

    /// Stops following the operation center and every recorded install.
    public func stop() {
        follower?.cancel()
        follower = nil
        for watcher in watchers.values { watcher.cancel() }
        watchers.removeAll()
    }

    /// Reads the first page again for the installs that have not ended, and takes any
    /// runtime it carries.
    func reloadInstalling() async {
        guard let reply = try? await Self.page(after: nil, call: call) else { return }
        held.installing = reply.installing ?? []
        for item in reply.items ?? [] { store.offer(item) }
    }

    /// Remembers an install the person started and follows its operation to its end.
    ///
    /// When it ends, ``finishedInstalls`` gets a record, and a failure is kept for the
    /// Catalog detail panel.
    ///
    /// - Parameters:
    ///   - operationId: The `install_runtime` operation the daemon accepted.
    ///   - provider: The provider ID, for the failure of the operation.
    ///   - name: The name shown for the software until the install shows in ``installing``.
    ///   - version: The version shown until the install shows in ``installing``.
    public func recordStarted(
        operationId: UUID, provider: String? = nil, name: String? = nil, version: String? = nil
    ) {
        guard recorded[operationId] == nil else { return }
        recorded[operationId] = Recorded(provider: provider, name: name, version: version)
        watchers[operationId] = Task { [weak self, updates] in
            var last: OperationSnapshot?
            for await operation in updates(operationId) {
                last = operation
            }
            guard let self else { return }
            await self.finish(operationId, last: last)
        }
        Task { await reloadInstalling() }
    }

    /// Ends the record of `operationId` with its last known state.
    private func finish(_ operationId: UUID, last: OperationSnapshot?) async {
        defer {
            watchers[operationId] = nil
            recorded[operationId] = nil
        }
        guard let last, let record = recorded[operationId] else { return }
        let outcome: FinishedInstall.Outcome
        switch last.state {
        case .succeeded: outcome = .succeeded
        case .cancelled: outcome = .cancelled
        case .failed: outcome = .failed
        case .queued, .running, .cancelRequested, .needsRecovery: return
        }
        let running = held.installing.first { $0.operationId == operationId }
        let name = running?.displayName ?? record.name ?? "Runtime"
        let version = running?.version ?? record.version ?? ""
        if outcome == .failed, let error = last.failure?.error, let provider = record.provider {
            held.failures[provider] = RuntimeInstallFailure(provider: provider, error: error)
        }
        await reloadInstalling()
        continuation.yield(
            FinishedInstall(
                operationId: operationId, name: name, version: version, outcome: outcome))
    }

    // MARK: - Get

    /// The Get action of one artifact in the version matrix of the Catalog (spec 0018,
    /// *Kolom aksi Catalog*): `runtime.install` for `version` with the digest the person
    /// saw, so a catalog that moved on is refused rather than installed.
    ///
    /// A click while an earlier one for the same artifact has no answer sends nothing. A
    /// key whose outcome was unknown is reused for five minutes, so a retry cannot start a
    /// second install.
    ///
    /// - Parameters:
    ///   - provider: The provider ID.
    ///   - displayName: The name shown for the provider.
    ///   - version: The exact version.
    ///   - sha256: The digest of the artifact for this Mac.
    ///   - disabledReason: Why the gate of the catalog refuses it, or `nil`.
    ///   - context: Where it reports.
    public func getAction(
        provider: String, displayName: String, version: String, sha256: String,
        disabledReason: String?, context: InstanceActionContext
    ) -> StacklyAction {
        StacklyAction(
            id: "runtimeGet", title: "Get", mutates: true, disabledReason: disabledReason,
            isBusy: isStarting(sha256: sha256),
            perform: { [self] in
                Task {
                    await get(
                        provider: provider, displayName: displayName, version: version,
                        sha256: sha256, context: context)
                }
            })
    }

    /// Sends the install and reports what came of it.
    func get(
        provider: String, displayName: String, version: String, sha256: String,
        context: InstanceActionContext
    ) async {
        guard held.starting.insert(sha256).inserted else { return }
        defer { held.starting.remove(sha256) }
        held.failures[provider] = nil
        let key = keyForClick(sha256)
        let outcome = await send(
            .runtimeInstall(
                InstallRuntimeParams(provider: provider, selector: version, expectedSha256: sha256)
            ), key)
        switch outcome {
        case .completed(let reply):
            keys[sha256] = nil
            guard case .runtimeInstall(let installed) = reply else {
                held.failures[provider] = RuntimeInstallFailure(
                    provider: provider, error: ClientErrorCatalog.protocolViolation())
                return
            }
            switch installed.outcome {
            case .started(let accepted):
                recordStarted(
                    operationId: accepted.operation.id, provider: provider, name: displayName,
                    version: version)
                // Keep the button busy until the install shows in the list, so it does not
                // flash back to Get.
                await reloadInstalling()
                context.report(
                    .started(
                        "Installing \(displayName) \(version). Staged, verified, then committed."))
            case .alreadyInstalled(let summary):
                store.offer(summary)
                context.report(.neutral("\(displayName) \(version) is already installed."))
            }
        case .rejected(let body):
            keys[sha256] = nil
            held.failures[provider] = RuntimeInstallFailure(provider: provider, error: body)
        case .notSent:
            keys[sha256] = nil
            report(outcome, context: context)
        case .outcomeUnknown(_, let unknownKey):
            keys[sha256] = StoredKey(key: unknownKey, at: now())
            report(outcome, context: context)
        }
    }

    private func report(_ outcome: MutationOutcome, context: InstanceActionContext) {
        if let feedback = Feedback.outcome(
            outcome, success: nil, showOperations: context.showOperations)
        {
            context.report(feedback)
        }
    }

    /// The key for a click: the stored key of an unknown outcome while it is fresh, else a
    /// new one.
    private func keyForClick(_ sha256: String) -> String {
        if let stored = keys[sha256], now().timeIntervalSince(stored.at) < Self.keyLifetime {
            return stored.key
        }
        return MutationPath.newKey()
    }

    // MARK: - Rules

    /// By name with the Finder's rules, then the newest version first, then ID.
    static func order(_ a: RuntimeSummary, _ b: RuntimeSummary) -> Bool {
        switch a.displayName.localizedStandardCompare(b.displayName) {
        case .orderedAscending: return true
        case .orderedDescending: return false
        case .orderedSame:
            switch a.version.compare(b.version, options: .numeric) {
            case .orderedDescending: return true
            case .orderedAscending: return false
            case .orderedSame: return a.id.uuidString < b.id.uuidString
            }
        }
    }

    /// What a `runtime.` event does to the store. The event has no payload and always
    /// carries a new revision, so the runtime is fetched again (spec 0018, K18).
    static func reduce(_ event: Event) -> StoreEffect<RuntimeSummary> {
        guard case .runtimeStateChanged = event.kind else { return .ignore }
        guard let revision = event.revision,
            let id = event.resource.flatMap({ UUID(uuidString: $0.id) })
        else { return .ignore }
        return .refetch(id, revision: revision)
    }

    private static func pageToken(_ id: UUID) -> PageToken {
        PageToken(id.uuidString.lowercased())
    }

    // MARK: - Calls

    /// One page of `runtime.list`, after `token`.
    static func page(
        after token: PageToken?,
        call: (Call) async throws(ClientError) -> Reply
    ) async throws(ClientError) -> RuntimeListPage {
        let params = ListRuntimesParams(
            limit: pageLimit, after: token.flatMap { UUID(uuidString: $0.value) })
        guard case .runtimeList(let page) = try await call(.runtimeList(params)) else {
            throw .rejected(ClientErrorCatalog.protocolViolation())
        }
        return page
    }

    /// One runtime from `runtime.get`, or `nil` when the daemon answers
    /// `resource.not_found`.
    static func runtime(
        _ id: UUID,
        call: (Call) async throws(ClientError) -> Reply
    ) async throws(ClientError) -> RuntimeSummary? {
        let reply: Reply
        do throws(ClientError) {
            reply = try await call(.runtimeGet(GetRuntimeParams(runtimeId: id)))
        } catch {
            if case .rejected(let body) = error, body.code == "resource.not_found" { return nil }
            throw error
        }
        guard case .runtimeGet(let detail) = reply else {
            throw .rejected(ClientErrorCatalog.protocolViolation())
        }
        return detail.summary
    }
}
