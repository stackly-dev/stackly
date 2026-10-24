import StacklyClient

/// The readiness of an instance in words (spec 0021, *Format bersama*), for the inspector
/// of the Instances page and later for the detail page. A pure function of the summary.
nonisolated public enum ReadinessText {
    /// The readiness text.
    /// Only an instance that is `running` or `recovering` has a readiness worth showing; any
    /// other instance gets "—".
    /// - Parameter instance: The instance.
    public static func text(for instance: InstanceSummary) -> String {
        switch instance.lifecycle {
        case .running, .recovering:
            break
        case .uninitialized, .initializing, .stopped, .starting, .stopping, .failed, .unknown,
            .retained:
            return "—"
        }
        let readiness = instance.readiness
        switch readiness.state {
        case .ready:
            return "Ready"
        case .notReady:
            if readiness.reason == StatusAdapter.awaitingProbeReason { return "Waiting for probe" }
            return readiness.detail.map { "Not ready: \($0)" } ?? "Not ready"
        case .unknown:
            return "Unknown (\(readiness.reason))"
        }
    }
}
