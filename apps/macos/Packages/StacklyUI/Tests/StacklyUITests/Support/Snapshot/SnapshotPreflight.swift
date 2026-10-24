import CoreGraphics

/// Checks that a snapshot has a GUI login session.
nonisolated enum SnapshotPreflight {
    /// Facts supplied by the host or a test.
    struct Facts: Sendable {
        var hasGUISession: Bool

        /// Reads the host session.
        @MainActor static func current() -> Facts {
            Facts(hasGUISession: CGSessionCopyCurrentDictionary() != nil)
        }
    }

    /// Why rendering is blocked, or `nil` when it can run.
    static func blockedReason(_ facts: Facts) -> String? {
        facts.hasGUISession ? nil : "no GUI login session"
    }
}
