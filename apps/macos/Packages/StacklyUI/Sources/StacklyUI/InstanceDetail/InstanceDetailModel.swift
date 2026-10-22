import Foundation
import Observation
import StacklyClient

/// What the detail page holds for one instance (spec 0021, *Tab Summary*): the detail and
/// the data size the daemon last sent, whether each is loading or failed, and the timers
/// that reload the detail and announce that the instance is ready.
///
/// The instance facts that change often (lifecycle, process, readiness) are not here; the
/// page reads them from the ``InstanceStore`` summary and tells the model about each new
/// one. The model reloads the detail when the summary's `revision` moves (after a pause of
/// ``reloadDebounce``) and when a `secret.changed` event names one of the instance's
/// references, and it drops a reply whose `revision` is older than the detail it holds. It
/// never holds a secret value: a revealed value lives in ``RevealModel`` for one call.
@Observable
public final class InstanceDetailModel {
    /// The pause before the detail reloads after a revision change: 250 ms
    /// (`DETAIL_RELOAD_DEBOUNCE`).
    public static let reloadDebounce: Duration = .milliseconds(250)
    /// How long readiness must last before "is ready" is announced: 2 seconds
    /// (`READY_ANNOUNCE_DELAY`).
    public static let readyAnnounceDelay: Duration = .seconds(2)

    /// The instance this model is for.
    public let instanceId: UUID

    /// The last detail the daemon sent, kept while a reload runs or fails.
    public private(set) var detail: InstanceDetail?
    /// Whether the detail is being read.
    public private(set) var isLoadingDetail = false
    /// Why the last reload of the detail failed, or `nil`. The old detail stays.
    public private(set) var detailError: ErrorBody?
    /// Whether the daemon said the instance does not exist.
    public private(set) var isNotFound = false
    /// The data size the daemon last sent, or `nil`.
    public private(set) var usage: DataUsage?
    /// Whether the data size is being read.
    public private(set) var isLoadingUsage = false
    /// Whether the last read of the data size failed.
    public private(set) var usageFailed = false

    @ObservationIgnored private let service: InstanceDetailService
    @ObservationIgnored private let debounce: Duration
    @ObservationIgnored private let readyDelay: Duration
    @ObservationIgnored private let sleep: @Sendable (Duration) async throws -> Void
    @ObservationIgnored private let announce: @MainActor (String) -> Void
    @ObservationIgnored private var reloadTask: Task<Void, Never>?
    @ObservationIgnored private var readyTask: Task<Void, Never>?
    @ObservationIgnored private var requests = 0
    @ObservationIgnored private var usageRequests = 0
    @ObservationIgnored private var observedRevision: UInt64?
    @ObservationIgnored private var wasReady: Bool?

    /// Creates a model; nothing loads until ``open()``.
    ///
    /// - Parameters:
    ///   - instanceId: The instance.
    ///   - service: The reads and the event stream.
    ///   - debounce: The pause before a reload after a revision change.
    ///   - readyDelay: How long readiness must last before it is announced.
    ///   - sleep: Waits for a duration and throws when cancelled; tests inject one that does
    ///     not wait.
    ///   - announce: Posts a VoiceOver announcement.
    public init(
        instanceId: UUID,
        service: InstanceDetailService,
        debounce: Duration = InstanceDetailModel.reloadDebounce,
        readyDelay: Duration = InstanceDetailModel.readyAnnounceDelay,
        sleep: @escaping @Sendable (Duration) async throws -> Void = {
            try await Task.sleep(for: $0)
        },
        announce: @escaping @MainActor (String) -> Void = { StacklyUI.announce($0) }
    ) {
        self.instanceId = instanceId
        self.service = service
        self.debounce = debounce
        self.readyDelay = readyDelay
        self.sleep = sleep
        self.announce = announce
    }

    // MARK: - What the views read

    /// The state of the data size row.
    public var dataSizeState: DataSizeState {
        if isLoadingUsage { return .loading }
        if usageFailed { return .unavailable }
        return .loaded(usage)
    }

    /// Whether the page still has no detail and one is on its way: the rows that depend on
    /// the detail show a delayed spinner.
    public var isDetailPending: Bool {
        detail == nil && detailError == nil && isNotFound == false
    }

    // MARK: - Loading

    /// Reads the detail, then the data size, when the page appears. The size read starts a
    /// measurement the daemon may need a moment for, so it does not hold the detail back.
    public func open() async {
        async let size: Void = loadUsage(refresh: false)
        await reload()
        await size
    }

    /// Reads the detail and the size the daemon already holds, for the inspector of the
    /// Instances page: it never starts a measurement (`cachedOnly`), so the size shows "—"
    /// until the detail page or Recalculate measured it.
    public func openForInspector() async {
        async let size: Void = loadUsage(refresh: false, cachedOnly: true)
        await reload()
        await size
    }

    /// Reads the detail now. A reply older than the held detail is dropped.
    public func reload() async {
        requests += 1
        let mine = requests
        isLoadingDetail = true
        defer { if mine == requests { isLoadingDetail = false } }
        do throws(ClientError) {
            let reply = try await service.detail(instanceId)
            guard mine == requests, Task.isCancelled == false else { return }
            apply(reply)
        } catch {
            guard mine == requests, Task.isCancelled == false else { return }
            fail(error)
        }
    }

    private func apply(_ reply: InstanceDetail) {
        isNotFound = false
        if let held = detail, reply.revision < held.revision { return }
        detail = reply
        detailError = nil
    }

    private func fail(_ error: ClientError) {
        let body = error.body
        if body.code == "resource.not_found" {
            isNotFound = true
            return
        }
        detailError = body
    }

    /// Reads the data size. `refresh` skips the daemon's cache (Recalculate); `cachedOnly`
    /// never starts a measurement. The daemon refuses both together.
    ///
    /// - Parameters:
    ///   - refresh: Whether to measure again.
    ///   - cachedOnly: Whether to answer from the daemon's cache only.
    public func loadUsage(refresh: Bool, cachedOnly: Bool = false) async {
        usageRequests += 1
        let mine = usageRequests
        isLoadingUsage = true
        defer { if mine == usageRequests { isLoadingUsage = false } }
        do throws(ClientError) {
            let reply = try await service.usage(instanceId, refresh, cachedOnly)
            guard mine == usageRequests, Task.isCancelled == false else { return }
            usage = reply
            usageFailed = false
        } catch {
            guard mine == usageRequests, Task.isCancelled == false else { return }
            usageFailed = true
        }
    }

    // MARK: - Following the instance

    /// Tells the model about the summary now: it reloads the detail after a pause when the
    /// revision moved, and it announces that the instance is ready once readiness lasted
    /// ``readyAnnounceDelay`` (never for an instance that was already ready on arrival).
    ///
    /// - Parameters:
    ///   - summary: The summary in the instance store.
    ///   - announcesReady: Whether readiness is announced. The detail page does; the inspector
    ///     of the Instances page only keeps the detail current.
    public func observe(_ summary: InstanceSummary, announcesReady: Bool = true) {
        if observedRevision != summary.revision {
            let first = observedRevision == nil
            observedRevision = summary.revision
            if first == false, detail?.revision != summary.revision {
                scheduleReload()
            }
        }
        guard announcesReady else { return }
        let ready = summary.lifecycle == .running && summary.readiness.state == .ready
        defer { wasReady = ready }
        guard let wasReady else { return }
        if ready == false {
            readyTask?.cancel()
            readyTask = nil
        } else if wasReady == false {
            scheduleReadyAnnouncement(name: summary.name)
        }
    }

    private func scheduleReload() {
        reloadTask?.cancel()
        let pause = debounce
        let sleep = sleep
        reloadTask = Task { [weak self] in
            do {
                try await sleep(pause)
            } catch {
                return
            }
            await self?.reload()
        }
    }

    private func scheduleReadyAnnouncement(name: String) {
        readyTask?.cancel()
        let delay = readyDelay
        let sleep = sleep
        readyTask = Task { [weak self] in
            do {
                try await sleep(delay)
            } catch {
                return
            }
            guard let self, Task.isCancelled == false else { return }
            announce("\(name) is ready.")
        }
    }

    /// Follows the `secret.` events until the task is cancelled: an event for one of the
    /// instance's references reloads the detail, since the status of a secret row changed.
    public func watchSecrets() async {
        for await event in service.secretEvents() {
            guard case .secretChanged = event.kind, let reference = event.resource?.id,
                detail?.secrets.contains(where: { $0.reference == reference }) == true
            else { continue }
            await reload()
        }
    }

    /// Stops the timers: the page went away.
    public func close() {
        reloadTask?.cancel()
        readyTask?.cancel()
        reloadTask = nil
        readyTask = nil
    }
}
