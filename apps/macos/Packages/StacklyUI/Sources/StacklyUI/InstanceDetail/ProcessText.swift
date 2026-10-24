import StacklyClient

/// The process of an instance in words (spec 0021, *Format bersama*): the "Process" row of
/// the Summary tab. A pure function of the summary, apart from the readiness text of
/// ``ReadinessText``, so the two never merge into one word.
nonisolated public enum ProcessText {
    /// The process text.
    /// - Parameter instance: The instance.
    /// - Returns: "Running (PID 4001)" with ", adopted" for a process the daemon found after
    ///   its own restart, or one word for the other lifecycle states.
    public static func text(for instance: InstanceSummary) -> String {
        switch instance.lifecycle {
        case .running:
            guard let process = instance.process else { return "Running" }
            return "Running (PID \(process.pid)\(process.adopted ? ", adopted" : ""))"
        case .recovering: return "Restarting"
        case .starting: return "Starting"
        case .stopping: return "Stopping"
        case .stopped: return "Stopped"
        case .failed: return "Failed"
        case .unknown: return "Unknown"
        case .initializing: return "Initializing"
        case .uninitialized: return "Not initialized"
        case .retained: return "Removed"
        }
    }
}
