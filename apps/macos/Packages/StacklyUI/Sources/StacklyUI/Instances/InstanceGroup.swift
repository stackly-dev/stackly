import StacklyClient

/// The group an instance counts in on the Overview, in the Instances filter, and in
/// `stackly instance list` (spec 0022, *Kelompok status instance*).
/// Only the lifecycle and the readiness state decide the group, never the process, the
/// endpoints, or the restart budget, so an instance whose process lives but is not ready
/// is never counted as running. The shared data file `protocols/fixtures/instance/groups.json`
/// keeps this rule equal to the Rust one.
nonisolated public enum InstanceGroup: String, CaseIterable, Hashable, Sendable {
    /// Running and ready.
    case ready
    /// A process that lives or is being started or stopped, but is not ready.
    case notReady
    /// Not running.
    case stopped
    /// Failed.
    case failed
    /// The identity of the process is lost.
    case unknown

    /// The group of a lifecycle and a readiness state.
    /// - Parameters:
    ///   - lifecycle: The instance lifecycle.
    ///   - readiness: The readiness state.
    /// - Returns: The group, or `nil` for a retained instance, which is not in any list.
    public static func of(
        lifecycle: InstanceLifecycle, readiness: InstanceReadinessState
    ) -> InstanceGroup? {
        switch lifecycle {
        case .running: readiness == .ready ? .ready : .notReady
        case .initializing, .starting, .recovering, .stopping: .notReady
        case .stopped, .uninitialized: .stopped
        case .failed: .failed
        case .unknown: .unknown
        case .retained: nil
        }
    }

    /// The group of an instance, or `nil` when it is retained.
    public static func of(_ instance: InstanceSummary) -> InstanceGroup? {
        of(lifecycle: instance.lifecycle, readiness: instance.readiness.state)
    }

    /// The position in the Status column order: failed, unknown, not ready, ready, stopped.
    var statusRank: Int {
        switch self {
        case .failed: 0
        case .unknown: 1
        case .notReady: 2
        case .ready: 3
        case .stopped: 4
        }
    }
}

/// How many instances are in each group. `total` is the sum of the five groups.
nonisolated public struct InstanceCounts: Hashable, Sendable {
    /// Every instance that is in a group.
    public var total = 0
    /// Running and ready.
    public var ready = 0
    /// Alive or changing, but not ready.
    public var notReady = 0
    /// Not running.
    public var stopped = 0
    /// Failed.
    public var failed = 0
    /// Identity lost.
    public var unknown = 0

    /// Zero in every group.
    public init() {}

    /// Counts `instances`; a retained instance counts nowhere.
    public init(of instances: some Sequence<InstanceSummary>) {
        for instance in instances {
            guard let group = InstanceGroup.of(instance) else { continue }
            total += 1
            switch group {
            case .ready: ready += 1
            case .notReady: notReady += 1
            case .stopped: stopped += 1
            case .failed: failed += 1
            case .unknown: unknown += 1
            }
        }
    }

    /// The number of instances the filter lets through.
    public func count(in filter: InstanceFilter) -> Int {
        switch filter {
        case .all: total
        case .running: ready + notReady
        case .stopped: stopped
        case .failed: failed
        }
    }
}

/// The status filter of the Instances page: All, Running, Stopped, and Failed.
nonisolated public enum InstanceFilter: String, CaseIterable, Identifiable, Hashable, Sendable {
    /// Every instance, also the ones in the `unknown` group.
    case all
    /// `ready` and `notReady`.
    case running
    /// `stopped`.
    case stopped
    /// `failed`.
    case failed

    /// The filter's identity: itself.
    public var id: Self { self }

    /// The title of the segment.
    public var title: String {
        switch self {
        case .all: "All"
        case .running: "Running"
        case .stopped: "Stopped"
        case .failed: "Failed"
        }
    }

    /// Whether an instance in `group` shows under this filter.
    public func includes(_ group: InstanceGroup?) -> Bool {
        guard let group else { return false }
        switch self {
        case .all: return true
        case .running: return group == .ready || group == .notReady
        case .stopped: return group == .stopped
        case .failed: return group == .failed
        }
    }
}
