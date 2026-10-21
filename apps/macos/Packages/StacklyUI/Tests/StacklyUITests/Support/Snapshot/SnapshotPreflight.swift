import AppKit
import CoreGraphics
import Foundation

/// What stops a snapshot from being a fair test on this Mac (spec 0068, AC-4): no login
/// session, or a system accessibility setting that AppKit controls would follow.
nonisolated enum SnapshotPreflight {
    /// The system facts the check reads, so a test can supply others.
    struct Facts: Sendable {
        var hasGUISession: Bool
        var increaseContrast: Bool
        var reduceTransparency: Bool
        var differentiateWithoutColor: Bool
        var reduceMotion: Bool

        /// The facts of this Mac now.
        @MainActor
        static func current() -> Facts {
            let workspace = NSWorkspace.shared
            return Facts(
                hasGUISession: CGSessionCopyCurrentDictionary() != nil,
                increaseContrast: workspace.accessibilityDisplayShouldIncreaseContrast,
                reduceTransparency: workspace.accessibilityDisplayShouldReduceTransparency,
                differentiateWithoutColor:
                    workspace.accessibilityDisplayShouldDifferentiateWithoutColor,
                reduceMotion: workspace.accessibilityDisplayShouldReduceMotion)
        }
    }

    /// Why the snapshot is blocked, or `nil` when it can run.
    static func blockedReason(_ facts: Facts) -> String? {
        guard facts.hasGUISession else { return "no GUI login session" }
        let settings: [(Bool, String)] = [
            (facts.increaseContrast, "Increase Contrast"),
            (facts.reduceTransparency, "Reduce Transparency"),
            (facts.differentiateWithoutColor, "Differentiate Without Color"),
            (facts.reduceMotion, "Reduce Motion"),
        ]
        if let on = settings.first(where: { $0.0 }) {
            return "system accessibility setting on: \(on.1)"
        }
        return nil
    }
}
