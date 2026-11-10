import Foundation
import Observation
import StacklyClient

/// The Activity tab of one instance (spec 0035, AC-15): the rows of `audit.list { instanceId }`,
/// newest first, 50 per page with Show More.
///
/// While a view shows the tab the model loads the first page again when the tab appears, when
/// an operation whose resource is this instance changes state (several changes within 500 ms
/// make one load; the daemon publishes the event after the commit that wrote the row, spec
/// 0007), and when the shell completes a snapshot round (``reconnected()``). Audit rows never
/// change once written, so a new first page joins the rows already loaded where it meets them,
/// and the pages Show More loaded stay. A daemon without `audit.list` makes the tab
/// unsupported, as ``ResourceStore`` does for its stores (spec 0013).
@Observable
final class InstanceActivityModel {
    /// The rows of one page (AC-15).
    nonisolated static let pageSize: UInt32 = 50
    /// How long the model gathers operation changes before it loads again.
    nonisolated static let eventDelay: Duration = .milliseconds(500)

    /// The instance.
    let instanceId: UUID

    /// The rows, newest first by `(occurredAt, id)`, the order of `audit.list`.
    private(set) var items: [AuditEventInfo] = []
    /// The cursor of the next page, or `nil` when the loaded rows are the whole history.
    private(set) var next: String?
    /// When the first page last loaded, or `nil` before it did.
    private(set) var loadedAt: Date?
    /// The error of the last load of the first page, or `nil` after one that worked.
    private(set) var error: ErrorBody?
    /// Whether the daemon has no `audit.list` (`request.unknown_method`).
    private(set) var unsupported = false
    /// Whether the first page is loading.
    private(set) var isRefreshing = false
    /// Whether Show More is loading the next page.
    private(set) var isLoadingMore = false
    /// The error of the last Show More, or `nil`.
    private(set) var moreError: ErrorBody?

    @ObservationIgnored private let read:
        @Sendable (UUID, String?) async throws(ClientError) ->
            AuditPage
    @ObservationIgnored private let now: () -> Date
    @ObservationIgnored private let sleep: @Sendable (Duration) async throws -> Void
    @ObservationIgnored private var viewers = 0
    /// Counts the loads of the first page, so an older answer never replaces a newer one.
    @ObservationIgnored private var generation = 0
    @ObservationIgnored private var following: Task<Void, Never>?
    @ObservationIgnored private var pending: Task<Void, Never>?
    /// The last state seen of each operation of this instance, so a progress event alone
    /// does not load the page again.
    @ObservationIgnored private var states: [UUID: OperationState] = [:]

    /// Creates the model of one instance's Activity tab; nothing is read until ``open(changes:)``.
    /// - Parameters:
    ///   - instanceId: The instance.
    ///   - read: `audit.list` with the instance and a cursor (`nil` for the first page).
    ///   - now: The wall clock.
    ///   - sleep: Waits for a duration and throws when cancelled.
    init(
        instanceId: UUID,
        read: @escaping @Sendable (UUID, String?) async throws(ClientError) -> AuditPage,
        now: @escaping () -> Date = { Date() },
        sleep: @escaping @Sendable (Duration) async throws -> Void = {
            try await Task.sleep(for: $0)
        }
    ) {
        self.instanceId = instanceId
        self.read = read
        self.now = now
        self.sleep = sleep
    }

    /// The model of one instance over the `activity` read of a detail service.
    /// - Parameters:
    ///   - instanceId: The instance.
    ///   - service: The detail service of the page.
    convenience init(instanceId: UUID, service: InstanceDetailService) {
        self.init(instanceId: instanceId, read: service.activity)
    }

    isolated deinit {
        following?.cancel()
        pending?.cancel()
    }

    // MARK: - Lifecycle

    /// A view shows the tab: loads the first page and follows the operation changes of the
    /// window, when it has an operation center.
    /// - Parameter changes: The changes of the window's ``OperationCenter``, or `nil`.
    func open(changes: AsyncStream<OperationCenterChange>?) {
        viewers += 1
        guard viewers == 1 else { return }
        if let changes {
            following = Task { [weak self] in
                for await change in changes {
                    guard let self else { return }
                    if case .operation(let operation) = change {
                        receive(operation)
                    }
                }
            }
        }
        Task { await reload() }
    }

    /// A view stopped showing the tab. When none is left, the model stops following the
    /// changes; the rows stay.
    func close() {
        viewers = max(0, viewers - 1)
        guard viewers == 0 else { return }
        following?.cancel()
        following = nil
        pending?.cancel()
        pending = nil
        states = [:]
    }

    /// The shell completed a snapshot round: a tab on screen loads its first page again.
    func reconnected() {
        guard viewers > 0 else { return }
        Task { await reload() }
    }

    // MARK: - Events

    /// Notes a change of an operation: a new state of an operation of this instance loads the
    /// first page within ``eventDelay``, together with every other change in that time.
    /// - Parameter operation: The operation as the center holds it now.
    func receive(_ operation: OperationSnapshot) {
        guard Self.isAbout(operation, instance: instanceId) else { return }
        guard states[operation.id] != operation.state else { return }
        states[operation.id] = operation.state
        guard pending == nil else { return }
        pending = Task { [weak self, sleep = self.sleep] in
            do {
                try await sleep(Self.eventDelay)
            } catch {
                return
            }
            guard let self else { return }
            pending = nil
            await reload()
        }
    }

    /// Whether the resource of `operation` is the instance `id`.
    nonisolated static func isAbout(_ operation: OperationSnapshot, instance id: UUID) -> Bool {
        guard let resource = operation.resource, resource.kind == .instance else { return false }
        return UUID(uuidString: resource.id) == id
    }

    // MARK: - Loading

    /// Loads the first page. Rows already loaded stay below it when it reaches them; else it
    /// replaces them, with its own cursor.
    func reload() async {
        generation += 1
        let generation = generation
        isRefreshing = true
        let page: AuditPage
        do throws(ClientError) {
            page = try await read(instanceId, nil)
        } catch {
            guard generation == self.generation else { return }
            isRefreshing = false
            if error.body.code == "request.unknown_method" {
                unsupported = true
                self.error = nil
            } else {
                self.error = error.body
            }
            return
        }
        guard generation == self.generation else { return }
        isRefreshing = false
        unsupported = false
        error = nil
        moreError = nil
        loadedAt = now()
        let fresh = page.items ?? []
        if page.next != nil, let last = fresh.last,
            let index = items.firstIndex(where: { $0.id == last.id })
        {
            let known = Set(fresh.map(\.id))
            items = fresh + items[(index + 1)...].filter { known.contains($0.id) == false }
        } else {
            items = fresh
            next = page.next
        }
    }

    /// Loads the page after the loaded rows (Show More).
    func showMore() async {
        guard let cursor = next, isLoadingMore == false else { return }
        let generation = generation
        isLoadingMore = true
        defer { isLoadingMore = false }
        let page: AuditPage
        do throws(ClientError) {
            page = try await read(instanceId, cursor)
        } catch {
            guard generation == self.generation else { return }
            moreError = error.body
            return
        }
        guard generation == self.generation, next == cursor else { return }
        moreError = nil
        next = page.next
        let known = Set(items.map(\.id))
        items += (page.items ?? []).filter { known.contains($0.id) == false }
    }

    // MARK: - Navigation

    /// The operation a selection of rows opens: the one of the first selected row that has
    /// one (AC-13), or `nil`.
    /// - Parameter selection: The IDs of the selected rows.
    func operation(for selection: Set<UUID>) -> UUID? {
        items.first { selection.contains($0.id) && $0.operationId != nil }?.operationId
    }
}
