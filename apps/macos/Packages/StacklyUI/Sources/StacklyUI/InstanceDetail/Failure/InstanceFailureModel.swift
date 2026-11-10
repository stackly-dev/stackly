import Foundation
import Observation
import StacklyClient

/// The operations the failure banner of one instance reads (spec 0033, *Sumber data*): one
/// `operation.list { resource, limit: 10 }` when the banner first shows, read again when the
/// summary's `revision` or `statusSeq` changes and when the operation center sees an operation
/// of the instance end. Between reads, the center's entries for the instance replace what was
/// read, so a newer start or stop counts at once.
///
/// Until a first read succeeded (it runs, it failed, or the daemon is offline) the banner gets
/// no operations and shows the summary alone; a later read that fails keeps what the last one
/// read. The Summary tab and the inspector share one model per instance.
@Observable
public final class InstanceFailureModel {
    /// How many operations one read asks for.
    public static let readLimit: UInt32 = 10

    /// The instance.
    public let instanceId: UUID
    /// The operations of the last read that succeeded, newest first, or `nil` before one did.
    public private(set) var loaded: [OperationSummary]?
    /// Whether a read is on its way.
    public private(set) var isLoading = false
    /// Why the last read failed, or `nil` once one succeeded.
    public private(set) var readError: ErrorBody?

    @ObservationIgnored private let read:
        @Sendable (UUID, UInt32) async throws(ClientError) -> [OperationSummary]
    @ObservationIgnored private var requests = 0
    @ObservationIgnored private var didOpen = false
    @ObservationIgnored private var observed: ObservedSummary?
    @ObservationIgnored private var endedSeen: [UUID: UInt64] = [:]
    @ObservationIgnored private var reloadTask: Task<Void, Never>?

    /// What a change of the summary is compared on.
    private struct ObservedSummary: Equatable {
        let revision: UInt64
        let statusSeq: UInt64
    }

    /// Creates a model that has read nothing.
    /// - Parameters:
    ///   - instanceId: The instance.
    ///   - read: `operation.list` with the instance as resource and a limit.
    public init(
        instanceId: UUID,
        read: @escaping @Sendable (UUID, UInt32) async throws(ClientError) -> [OperationSummary]
    ) {
        self.instanceId = instanceId
        self.read = read
    }

    /// A model over the reads of the detail page.
    /// - Parameters:
    ///   - instanceId: The instance.
    ///   - service: The service whose `operations` read it uses.
    public convenience init(instanceId: UUID, service: InstanceDetailService) {
        self.init(instanceId: instanceId, read: service.operations)
    }

    // MARK: - Reading

    /// Reads the operations once, when the banner first shows; a model that already opened
    /// keeps what it read.
    public func open() async {
        guard didOpen == false else { return }
        didOpen = true
        await reload()
    }

    /// Reads the operations now. A reply to an older request is dropped.
    public func reload() async {
        didOpen = true
        requests += 1
        let mine = requests
        isLoading = true
        defer { if mine == requests { isLoading = false } }
        do throws(ClientError) {
            let items = try await read(instanceId, Self.readLimit)
            guard mine == requests, Task.isCancelled == false else { return }
            loaded = RelatedOperation.newestFirst(items)
            readError = nil
        } catch {
            guard mine == requests, Task.isCancelled == false else { return }
            readError = error.body
        }
    }

    // MARK: - Following

    /// Reads again when the summary's `revision` or `statusSeq` changed since the last one
    /// observed, while the instance can have a banner (`failed`, `stopped`, or `recovering`):
    /// a running instance changes its status often and never shows one.
    /// - Parameter summary: The summary in the instance store.
    public func observe(_ summary: InstanceSummary) {
        let now = ObservedSummary(revision: summary.revision, statusSeq: summary.statusSeq)
        defer { self.observed = now }
        guard let observed, observed != now, didOpen, Self.canHaveBanner(summary) else { return }
        scheduleReload()
    }

    /// Reads again when the center holds an ended operation of the instance that the last
    /// read does not hold at that revision.
    /// - Parameter center: The active and the recent operations of the operation center.
    public func observe(center: [OperationSummary]) {
        var fresh = false
        for operation in own(center) where OperationCenter.isActive(operation.state) == false {
            if let seen = endedSeen[operation.id], seen >= operation.revision { continue }
            endedSeen[operation.id] = operation.revision
            let held = loaded?.first { $0.id == operation.id }
            if (held?.revision ?? 0) < operation.revision { fresh = true }
        }
        // The first read is still on its way and will hold it.
        guard fresh, didOpen, loaded != nil || isLoading == false else { return }
        scheduleReload()
    }

    /// The operations the banner reads: what the last read held, with the center's entries
    /// for the instance in place of older ones, newest first; empty before a first read
    /// succeeded.
    /// - Parameter center: The active and the recent operations of the operation center.
    public func operations(center: [OperationSummary]) -> [OperationSummary] {
        guard let loaded else { return [] }
        var byID = Dictionary(loaded.map { ($0.id, $0) }, uniquingKeysWith: { first, _ in first })
        for operation in own(center) {
            if let held = byID[operation.id], held.revision >= operation.revision { continue }
            byID[operation.id] = operation
        }
        return RelatedOperation.newestFirst(Array(byID.values))
    }

    /// The banner of the instance from its summary and the operations this model holds.
    /// - Parameters:
    ///   - summary: The summary in the instance store.
    ///   - center: The active and the recent operations of the operation center.
    ///   - instanceNames: The name of another instance by ID.
    ///   - now: The clock of the restart countdown.
    public func content(
        for summary: InstanceSummary, center: [OperationSummary],
        instanceNames: (UUID) -> String?, now: Date
    ) -> FailureBannerContent? {
        FailureBannerContent.resolve(
            summary, related: operations(center: center), instanceNames: instanceNames, now: now)
    }

    /// Stops a read that waits to start: the page went away.
    public func close() {
        reloadTask?.cancel()
        reloadTask = nil
    }

    /// Waits for the read a change scheduled, for tests.
    func settle() async {
        await reloadTask?.value
    }

    // MARK: - Pieces

    private func scheduleReload() {
        reloadTask?.cancel()
        reloadTask = Task { [weak self] in
            await self?.reload()
        }
    }

    private func own(_ operations: [OperationSummary]) -> [OperationSummary] {
        let id = instanceId.uuidString.lowercased()
        return operations.filter {
            $0.resource?.kind == .instance && $0.resource?.id.lowercased() == id
        }
    }

    private static func canHaveBanner(_ summary: InstanceSummary) -> Bool {
        switch summary.lifecycle {
        case .failed, .stopped, .recovering: true
        default: false
        }
    }
}
