import StacklyClient

/// The stages of the operations that have a definition (spec 0022, *Seksi Operations*): the
/// five of `install_runtime` (spec 0018), the six of `create_instance` (spec 0020), and those
/// of start, stop, restart, force stop, apply, change ports, and drift (spec 0035, *Definisi
/// tahap*), whose keys are the `StageName` constants their supervisor jobs enter. Another kind
/// has none (spec 0022: the kinds of features 50 and 52, rename, startup, settings, stop all,
/// and the daemon restart, which have only two stages); its owner writes the definition when
/// it builds the operation.
nonisolated public enum OperationStages {
    /// The title of `backoff` (spec 0033): the respawn waits before it starts again.
    static let waitingToRestart = StageAlias(key: "backoff", title: "Waiting to restart")

    /// The stages of an operation kind, or `nil` when the kind has no definition.
    public static func definitions(for kind: OperationKind) -> [StageDefinition]? {
        switch kind {
        case .installRuntime:
            [
                StageDefinition(key: "download", title: "Download"),
                StageDefinition(key: "verify_digest", title: "Verify digest"),
                StageDefinition(key: "extract", title: "Extract"),
                StageDefinition(key: "validate_binary", title: "Validate binary"),
                StageDefinition(key: "commit", title: "Commit"),
            ]
        case .createInstance:
            [
                StageDefinition(key: "validate", title: "Validate"),
                StageDefinition(key: "pin", title: "Pin"),
                StageDefinition(key: "secrets", title: "Secrets"),
                StageDefinition(key: "layout", title: "Layout"),
                StageDefinition(key: "register", title: "Register"),
                StageDefinition(key: "init", title: "Initialize"),
            ]
        case .startInstance:
            [
                StageDefinition(
                    key: "validate", title: "Check",
                    also: ["journal", "lock", "verify", "endpoints"]),
                StageDefinition(key: "render", title: "Render config"),
                StageDefinition(
                    key: "spawn", title: "Launch", also: ["identity", waitingToRestart]),
                StageDefinition(key: "readiness", title: "Readiness"),
                StageDefinition(key: "publish", title: "Finish", also: ["complete"]),
            ]
        case .stopInstance:
            [
                StageDefinition(key: "validate", title: "Check", also: ["lock"]),
                StageDefinition(key: "journal", title: "Prepare"),
                StageDefinition(key: "stop", title: "Stop"),
                StageDefinition(key: "complete", title: "Finish"),
            ]
        case .restartInstance:
            [
                StageDefinition(key: "validate", title: "Check", also: ["lock", "journal"]),
                StageDefinition(key: "stop", title: "Stop", also: ["await_exit", "lock_wait"]),
                StageDefinition(
                    key: "start", title: "Start",
                    also: ["render", "spawn", "identity", waitingToRestart]),
                StageDefinition(key: "readiness", title: "Readiness"),
                StageDefinition(key: "publish", title: "Finish", also: ["complete"]),
            ]
        case .forceStopInstance:
            [
                StageDefinition(key: "validate", title: "Check", also: ["lock", "journal"]),
                StageDefinition(key: "kill", title: "Kill"),
                StageDefinition(key: "await_exit", title: "Wait for exit"),
                StageDefinition(key: "complete", title: "Finish"),
            ]
        case .applyConfig, .changeEndpoints:
            [
                StageDefinition(key: "validate", title: "Check", also: ["lock"]),
                StageDefinition(key: "commit", title: "Save revision"),
                StageDefinition(key: "apply", title: "Apply"),
                StageDefinition(key: "verify", title: "Verify"),
            ]
        case .resolveConfigDrift:
            [
                StageDefinition(key: "validate", title: "Check"),
                StageDefinition(key: "render", title: "Rewrite files"),
                StageDefinition(key: "verify", title: "Verify"),
            ]
        default:
            nil
        }
    }

    /// The label of the stage an operation is at: the title its definition gives it (the
    /// alias title for a key in ``StageDefinition/also``), else the key with the underscores
    /// replaced by spaces and a capital first letter ("verify_digest" is "Verify digest"), or
    /// the label of the state when there is no stage.
    public static func label(for operation: OperationSummary) -> String {
        guard let stage = operation.stage, stage.isEmpty == false else {
            return StatusAdapter.appearance(of: operation).label
        }
        if let definitions = definitions(for: operation.kind),
            let known = StagedProgressValue.stage(of: stage, in: definitions)
        {
            return known.title
        }
        let words = stage.replacingOccurrences(of: "_", with: " ")
        return words.prefix(1).uppercased() + words.dropFirst()
    }
}
