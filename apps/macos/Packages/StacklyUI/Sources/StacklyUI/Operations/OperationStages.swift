import StacklyClient

/// The stages of the operations that have a definition so far (spec 0022, *Seksi
/// Operations*): the five of `install_runtime` (spec 0018) and the six of `create_instance`
/// (spec 0020). Another kind has none yet; its owner (features 35, 50, 52) writes the
/// definition when it builds the operation.
nonisolated public enum OperationStages {
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
        default:
            nil
        }
    }

    /// The label of a stage without a definition: the key with the underscores replaced by
    /// spaces and a capital first letter ("verify_digest" is "Verify digest"), or the label
    /// of the state when there is no stage.
    public static func label(for operation: OperationSummary) -> String {
        guard let stage = operation.stage, stage.isEmpty == false else {
            return StatusAdapter.appearance(of: operation).label
        }
        let words = stage.replacingOccurrences(of: "_", with: " ")
        return words.prefix(1).uppercased() + words.dropFirst()
    }
}
