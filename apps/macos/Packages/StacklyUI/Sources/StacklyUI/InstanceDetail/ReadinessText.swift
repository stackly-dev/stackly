import StacklyClient

/// The readiness of an instance in words (spec 0021, *Format bersama*), for the inspector
/// of the Instances page and later for the detail page. A pure function of the summary.
nonisolated public enum ReadinessText {
    /// The readiness text, with the time the probe took when the daemon measured it: "Ready ·
    /// 3 ms" (spec 0028, AC-7).
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
        let words: String =
            switch readiness.state {
            case .ready:
                "Ready"
            case .notReady:
                readiness.reason == StatusAdapter.awaitingProbeReason
                    ? "Waiting for probe"
                    : readiness.detail.map { "Not ready: \($0)" } ?? "Not ready"
            case .unknown:
                "Unknown (\(readiness.reason))"
            }
        return readiness.latencyMs.map { "\(words) · \($0) ms" } ?? words
    }
}
