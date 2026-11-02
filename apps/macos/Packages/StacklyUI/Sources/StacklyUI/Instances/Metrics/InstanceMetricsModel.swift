import Foundation
import Observation
import StacklyClient

/// The Metrics tab of one instance (spec 0028, *Layar Metrics*, AC-13): the history of
/// samples, the process list, and the facts around them, read from `instance.get_metrics`.
///
/// While a view shows the tab the model asks the daemon every `intervalSeconds` seconds, with
/// the `historyId` and the newest `seq` it holds, so only newer samples travel. It replaces
/// the whole history when the reply says `historyReset`, keeps the last process list when a
/// reply brings no new sample (the daemon sends the list only with a new sample), and drops
/// everything when the instance is not running. When the connection closes the data stays and
/// the polling waits for ``reconnected()``.
@Observable
final class InstanceMetricsModel {
    /// Which processes the cards and the graph show.
    enum Scope: String, CaseIterable, Identifiable, Sendable {
        case main
        case tree

        var id: String { rawValue }

        /// The title in the segmented control.
        var title: String {
            switch self {
            case .main: "Main Process"
            case .tree: "Process Tree"
            }
        }
    }

    /// Where the tab stands.
    enum Phase: Equatable, Sendable {
        /// The first reply has not arrived.
        case loading
        /// A reply arrived.
        case ready
        /// The first read was refused or could not be understood, with why.
        case failed(ErrorBody)
    }

    /// The seconds between two reads until the first reply says otherwise.
    static let defaultIntervalSeconds: UInt32 = 5

    /// The instance.
    let instanceId: UUID

    /// The scope the cards and the graph show; the process list always shows the tree.
    var scope = Scope.main
    /// Where the tab stands.
    private(set) var phase = Phase.loading
    /// The newest reply: state, lifecycle, restarts, readiness, health, and data size.
    private(set) var reply: MetricsReply?
    /// The samples held, oldest first, at most `capacity`.
    private(set) var samples: [MetricsSample] = []
    /// The process list of the newest sample as a tree.
    private(set) var processes: [ProcessRow] = []
    /// True when the tree had more processes than the list holds.
    private(set) var processesTruncated = false
    /// Whether the connection closed under the tab; the data stays.
    private(set) var isOffline = false
    /// Why the last refresh failed after data had arrived, or `nil`.
    private(set) var refreshFailure: ErrorBody?

    @ObservationIgnored private let service: InstanceMetricsService
    @ObservationIgnored private let sleep: @Sendable (Duration) async throws -> Void
    @ObservationIgnored private var historyId: String?
    @ObservationIgnored private var generation = 0
    @ObservationIgnored private var viewers = 0
    @ObservationIgnored private var polling: Task<Void, Never>?

    /// Creates the model of one instance's Metrics tab; nothing is read until ``open()``.
    /// - Parameters:
    ///   - instanceId: The instance.
    ///   - service: The read.
    ///   - scope: The scope shown first.
    ///   - sleep: Waits for a duration and throws when cancelled.
    init(
        instanceId: UUID,
        service: InstanceMetricsService,
        scope: Scope = .main,
        sleep: @escaping @Sendable (Duration) async throws -> Void = {
            try await Task.sleep(for: $0)
        }
    ) {
        self.instanceId = instanceId
        self.service = service
        self.scope = scope
        self.sleep = sleep
    }

    // MARK: - Derived

    /// Whether the daemon says the instance has live metrics.
    var isLive: Bool { reply?.state == .live }

    /// The newest sample, or `nil` before the first one.
    var latestSample: MetricsSample? { samples.last }

    /// The usage of the selected scope in the newest sample.
    var latestUsage: ResourceUsage? {
        latestSample.map { scope == .main ? $0.main : $0.tree }
    }

    /// The seconds between two reads, from the newest reply.
    var intervalSeconds: UInt32 { reply?.intervalSeconds ?? Self.defaultIntervalSeconds }

    // MARK: - Lifecycle

    /// A view shows the tab: reads now and then every ``intervalSeconds`` seconds.
    func open() {
        viewers += 1
        guard viewers == 1 else { return }
        startPolling()
    }

    /// A view stopped showing the tab. When none is left, the polling stops; the data stays.
    func close() {
        viewers = max(0, viewers - 1)
        guard viewers == 0 else { return }
        stop()
    }

    /// Stops the polling whoever shows the tab, for an instance that is gone.
    func stop() {
        viewers = 0
        generation += 1
        polling?.cancel()
        polling = nil
    }

    /// Reads again after the first read failed ("Try Again").
    func retry() {
        guard viewers > 0 else { return }
        startPolling()
    }

    /// The shell caught up after a reconnect: a tab whose connection closed polls again.
    func reconnected() {
        guard isOffline, viewers > 0 else { return }
        startPolling()
    }

    // MARK: - Reads

    private func startPolling() {
        generation += 1
        polling?.cancel()
        isOffline = false
        if reply == nil { phase = .loading }
        let gen = generation
        polling = Task { [weak self] in
            while true {
                guard let self, Task.isCancelled == false, gen == self.generation else { return }
                guard await self.refresh(generation: gen), gen == self.generation else { return }
                let wait = Duration.seconds(Int(self.intervalSeconds))
                let sleep = self.sleep
                do { try await sleep(wait) } catch { return }
            }
        }
    }

    /// One read; `false` when polling must stop (the connection is closed).
    private func refresh(generation gen: Int) async -> Bool {
        let cursor = historyId
        let after = cursor == nil ? nil : samples.last?.seq
        let service = self.service
        let id = instanceId
        let result: Result<MetricsReply, ClientError>
        do throws(ClientError) {
            result = .success(try await service.read(id, cursor, after))
        } catch {
            result = .failure(error)
        }
        guard gen == generation else { return false }
        switch result {
        case .success(let reply):
            apply(reply)
            return true
        case .failure(.notSent), .failure(.outcomeUnknown):
            isOffline = true
            return false
        case .failure(.rejected(let body)):
            fail(body)
            return true
        }
    }

    private func fail(_ body: ErrorBody) {
        if reply == nil {
            phase = .failed(body)
        } else {
            refreshFailure = body
        }
    }

    /// Merges one reply into what the tab holds.
    private func apply(_ new: MetricsReply) {
        refreshFailure = nil
        phase = .ready
        reply = new
        if new.state != .live {
            historyId = nil
            samples = []
            processes = []
            processesTruncated = false
            return
        }
        let capacity = Int(clamping: new.capacity)
        let received = new.historyReset || new.historyId != historyId
        historyId = new.historyId
        var kept = received ? [] : samples
        let newest = kept.last?.seq ?? 0
        kept.append(contentsOf: new.samples.filter { received || $0.seq > newest })
        if capacity > 0, kept.count > capacity {
            kept.removeFirst(kept.count - capacity)
        }
        samples = kept
        if received || new.samples.isEmpty == false {
            processes = new.latestProcesses ?? []
            processesTruncated = new.processesTruncated ?? false
        }
    }
}
