import StacklyClient

/// The sentence for the last failure of an instance, from `lastFailure.code` (spec 0022,
/// *Copy kegagalan*). A code this build does not list gets "Failed with {code}.".
nonisolated public enum InstanceFailureCopy {
    /// The sentence for a failure.
    /// - Parameters:
    ///   - failure: The last failure, or `nil` when the daemon sent none.
    ///   - restartBudget: The restart budget, for a crash loop.
    public static func text(_ failure: InstanceLastFailure?, restartBudget: UInt32) -> String {
        guard let failure else { return "It failed." }
        switch failure.code {
        case "crash_loop":
            return "Crash loop after \(restartBudget) restarts."
        case "process_exited":
            return failure.exitCode.map { "Exited with code \($0)." } ?? "Its process exited."
        case "process_signaled":
            return failure.signal.map { "Stopped by signal \($0)." } ?? "Stopped by a signal."
        case "exit_status_unknown":
            return "Its process stopped unexpectedly."
        case "endpoint_in_use":
            return "Another process uses one of its ports."
        case "data_in_use":
            return "Another process holds its data."
        case "spawn_failed":
            return "macOS couldn’t start its process."
        case "executable_invalid":
            return "Its runtime executable failed verification."
        case "identity_capture_failed":
            return "Stackly couldn’t record its process."
        case "config_render_failed":
            return "Its configuration couldn’t be written."
        case "init_step_failed":
            return "Initialization failed."
        case "init_step_timeout", "init_timeout":
            return "Initialization took too long."
        case "interrupted":
            return "stacklyd stopped while it was changing state."
        case "quarantined":
            return "macOS quarantined its runtime."
        case "binary_rejected_by_os":
            return "macOS refused to run its runtime."
        default:
            return "Failed with \(failure.code)."
        }
    }

    /// The sentence for an instance: "Initialization failed." when its data folder was
    /// never initialized, otherwise the sentence of its last failure.
    public static func text(for instance: InstanceSummary) -> String {
        if instance.initialized == false { return "Initialization failed." }
        return text(instance.lastFailure, restartBudget: instance.restart.budget)
    }
}
