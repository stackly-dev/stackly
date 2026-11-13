import StacklyClient

/// Maps the daemon's status DTOs to the UI status enums of spec 0012 (spec 0013,
/// *Adapter DTO ke enum UI*).
/// Every mapping is one exhaustive `switch` over a closed protocol enum, without
/// `default`, so a new protocol case fails to compile until the UI knows how to show it.
/// The source rule test rejects `default:` anywhere in `Adapters/`.
nonisolated public enum StatusAdapter {
    /// The reason of a `not_ready` readiness that means the first probe has not run.
    static let awaitingProbeReason = "awaiting_probe"

    /// The UI status of an instance lifecycle.
    public static func lifecycle(_ lifecycle: InstanceLifecycle) -> LifecycleStatus {
        switch lifecycle {
        case .uninitialized: .uninitialized
        case .initializing: .initializing
        case .stopped: .stopped
        case .starting: .starting
        case .running: .running
        case .stopping: .stopping
        case .failed: .failed
        case .recovering: .recovering
        case .unknown: .unknown
        case .retained: .retained
        }
    }

    /// The UI status of an instance's readiness. `not_ready` with the reason
    /// `awaiting_probe` is "waiting for the first probe"; any other reason keeps the
    /// daemon's detail.
    public static func readiness(_ readiness: InstanceReadiness) -> ReadinessStatus {
        switch readiness.state {
        case .ready: .ready
        case .notReady:
            readiness.reason == awaitingProbeReason
                ? .awaitingProbe : .notReady(detail: readiness.detail)
        case .unknown: .unknown
        }
    }

    /// The UI status of an instance's health.
    public static func health(_ health: InstanceHealth) -> HealthStatus {
        switch health.state {
        case .unknown: .unknown
        case .healthy: .healthy
        case .degraded: .degraded
        case .unhealthy: .unhealthy
        }
    }

    /// How an instance's status looks in a row, from the instance status table of spec 0012.
    public static func appearance(of instance: InstanceSummary) -> StatusAppearance {
        .instance(
            lifecycle: lifecycle(instance.lifecycle),
            readiness: readiness(instance.readiness),
            health: health(instance.health))
    }

    /// The UI status of an operation state.
    public static func operation(_ state: OperationState) -> OperationStatus {
        switch state {
        case .queued: .queued
        case .running: .running
        case .cancelRequested: .cancelRequested
        case .succeeded: .succeeded
        case .failed: .failed
        case .cancelled: .cancelled
        case .needsRecovery: .needsRecovery
        }
    }

    /// Whether an operation finished only in part: its result lists items that did not
    /// all go to plan.
    public static func isPartial(_ operation: OperationSummary) -> Bool {
        operation.result?.partial?.isEmpty == false
    }

    /// The services an Up left ready, for an `environment_up` that failed with
    /// `env.up_incomplete` while at least one service is ready (spec 0046, *Activity*), or
    /// `nil`.
    public static func upIncomplete(_ operation: OperationSummary) -> (ready: Int, total: Int)? {
        guard operation.state == .failed, let error = operation.failure?.error,
            error.code == "env.up_incomplete"
        else { return nil }
        let details = Dictionary(
            (error.details ?? []).map { ($0.key, $0.value) },
            uniquingKeysWith: { first, _ in first })
        guard let ready = details["ready"].flatMap(Int.init), ready > 0,
            let total = details["total"].flatMap(Int.init)
        else { return nil }
        return (ready, total)
    }

    /// The "Partial" badge of an Up that left some services ready (spec 0046).
    public static let upPartial = StatusAppearance(
        label: "Partial", tone: .caution, symbol: Tone.caution.symbol)

    /// How an operation's status looks, from the operation status table of spec 0012, with
    /// "Partial" for an Up that left some services ready (spec 0046).
    public static func appearance(of operation: OperationSummary) -> StatusAppearance {
        if upIncomplete(operation) != nil { return upPartial }
        return .operation(state: self.operation(operation.state), partial: isPartial(operation))
    }
}
