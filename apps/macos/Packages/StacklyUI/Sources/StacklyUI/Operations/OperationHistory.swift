import Foundation
import Observation
import StacklyClient

/// The list of the Operations page (spec 0035, *Halaman Operations*): the history pages of
/// `operation.list` without `activeOnly`, plus every change the ``OperationCenter`` applies.
///
/// A change of an operation the list holds applies when its revision is higher. An operation
/// the list does not hold yet joins only when it is not older, by `(createdAt, id)`, than the
/// oldest row of the loaded pages, or when the loaded pages hold the whole history (no
/// `next`); an older one waits for Show More. An operation the page opens through
/// ``ShellNavigator/openOperation(_:)`` is read with `operation.get` (``read(_:)``) and joins in its place
/// whatever its age. Every snapshot the center loads again (a resnapshot of `ShellSync`) loads
/// the first page again.
@Observable
public final class OperationHistory {
    /// The rows, newest first by `(createdAt, id)`, the order of `operation.list`.
    public private(set) var items: [OperationSnapshot] = []
    /// The cursor of the next page, or `nil` when the loaded pages hold the whole history.
    public private(set) var next: String?
    /// When the first page last loaded, or `nil` before it did.
    public private(set) var loadedAt: Date?
    /// The error of the last load of the first page, or `nil` after one that worked.
    public private(set) var error: ErrorBody?
    /// Whether the first page is loading.
    public private(set) var isRefreshing = false
    /// Whether Show More is loading the next page.
    public private(set) var isLoadingMore = false
    /// The error of the last Show More, or `nil`.
    public private(set) var moreError: ErrorBody?
    /// The operations the window opened that the daemon no longer knows (spec 0035, AC-12).
    public private(set) var missing: Set<UUID> = []

    /// The oldest `(createdAt, id)` of the loaded pages: the bound below which an operation
    /// from an event waits for Show More.
    @ObservationIgnored private var oldestLoaded: OperationSnapshot?
    /// The operations the window opened, kept when the first page loads again.
    @ObservationIgnored private var opened: Set<UUID> = []
    /// Counts the loads of the first page, so an older answer never replaces a newer one.
    @ObservationIgnored private var generation = 0
    @ObservationIgnored private var following: Task<Void, Never>?
    @ObservationIgnored private let service: OperationsService
    @ObservationIgnored private let center: OperationCenter
    @ObservationIgnored private let now: () -> Date

    /// Creates an empty history; call ``start()`` to load it and follow the center.
    /// - Parameters:
    ///   - service: Reads the history pages and one operation.
    ///   - center: The operation center whose changes the history applies.
    ///   - now: The wall clock.
    public init(
        service: OperationsService, center: OperationCenter, now: @escaping () -> Date = { Date() }
    ) {
        self.service = service
        self.center = center
        self.now = now
    }

    isolated deinit {
        following?.cancel()
    }

    // MARK: - Loading

    /// Applies what the center holds, follows its changes, and loads the first page.
    /// Calling it again does nothing.
    public func start() async {
        guard following == nil else { return }
        let changes = center.changes()
        following = Task { [weak self] in
            for await change in changes {
                guard let self else { return }
                switch change {
                case .operation(let operation):
                    apply(operation)
                case .reloaded:
                    Task { await self.reload() }
                }
            }
        }
        for operation in center.active + center.recent {
            apply(operation)
        }
        await reload()
    }

    /// Stops following the center.
    public func stop() {
        following?.cancel()
        following = nil
    }

    /// Loads the first page again: it replaces the loaded pages, and the operations the window
    /// opened stay.
    public func reload() async {
        generation += 1
        let generation = generation
        isRefreshing = true
        let page: OperationPage
        do {
            page = try await service.list(nil)
        } catch {
            guard generation == self.generation else { return }
            isRefreshing = false
            self.error = error.body
            return
        }
        guard generation == self.generation else { return }
        isRefreshing = false
        error = nil
        moreError = nil
        loadedAt = now()
        let fresh = page.items ?? []
        let kept = items.filter { held in
            opened.contains(held.id)
                || OperationCenter.isActive(held.state)
                    && center.store.items[held.id] != nil
        }
        items = []
        next = page.next
        oldestLoaded = fresh.last
        for operation in fresh + kept {
            merge(operation)
        }
    }

    /// Loads the page after the loaded ones (Show More).
    public func showMore() async {
        guard let cursor = next, isLoadingMore == false else { return }
        let generation = generation
        isLoadingMore = true
        defer { isLoadingMore = false }
        let page: OperationPage
        do {
            page = try await service.list(cursor)
        } catch {
            guard generation == self.generation else { return }
            moreError = error.body
            return
        }
        guard generation == self.generation, next == cursor else { return }
        moreError = nil
        next = page.next
        let fresh = page.items ?? []
        if let last = fresh.last {
            oldestLoaded = last
        }
        for operation in fresh {
            merge(operation)
        }
    }

    /// Reads one operation with `operation.get` and puts it in its place, also when it is older
    /// than the loaded pages: the window opened it (spec 0035, AC-13). An operation the daemon
    /// no longer knows leaves the list and goes to ``missing``.
    /// - Parameter id: The operation.
    /// - Returns: The detail, or the error of the read.
    @discardableResult
    public func read(_ id: UUID) async -> Result<OperationDetail, ClientError> {
        opened.insert(id)
        do {
            let detail = try await service.get(id)
            missing.remove(id)
            merge(detail.operation)
            return .success(detail)
        } catch {
            if case .rejected(let body) = error, body.code == "resource.not_found" {
                missing.insert(id)
                items.removeAll { $0.id == id }
            }
            return .failure(error)
        }
    }

    // MARK: - Changes

    /// Applies a newer summary: from an event, a reply, or a read.
    /// - Parameter operation: The summary.
    public func apply(_ operation: OperationSnapshot) {
        if items.contains(where: { $0.id == operation.id }) || opened.contains(operation.id) {
            merge(operation)
        } else if next == nil || oldestLoaded.map({ Self.isNewer(operation, than: $0) }) ?? true {
            merge(operation)
        }
    }

    /// Puts `operation` in its place, or replaces the held one when its revision is higher.
    private func merge(_ operation: OperationSnapshot) {
        if let index = items.firstIndex(where: { $0.id == operation.id }) {
            guard operation.revision > items[index].revision else { return }
            items[index] = operation
            return
        }
        let index = items.firstIndex { Self.isNewer(operation, than: $0) } ?? items.endIndex
        items.insert(operation, at: index)
    }

    /// Whether `a` comes before `b` in the order of `operation.list`, or is `b` itself.
    nonisolated static func isNewer(_ a: OperationSnapshot, than b: OperationSnapshot) -> Bool {
        if a.createdAt != b.createdAt { return a.createdAt > b.createdAt }
        return a.id.uuidString.lowercased() >= b.id.uuidString.lowercased()
    }
}
