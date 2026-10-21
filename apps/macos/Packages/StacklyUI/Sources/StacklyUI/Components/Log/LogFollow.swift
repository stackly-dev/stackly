import SwiftUI

/// The rule that decides when a ``LogView`` stops following the newest line.
///
/// Kept apart from the view so the rule is tested without a scroll view.
nonisolated enum LogFollow {
    /// How far above the bottom a scroll by the user may end before follow turns off:
    /// 24 pt.
    static let bottomTolerance: CGFloat = 24

    /// Whether the scroll position, `distance` points above the bottom, still counts as
    /// the bottom.
    static func isAtBottom(distance: CGFloat) -> Bool {
        distance <= bottomTolerance
    }

    /// Whether a change of scroll phase is the end of a scroll by the user: from
    /// interacting or decelerating to idle. A scroll by the program animates or jumps
    /// without passing through those two phases, so it never ends follow.
    static func endsUserScroll(from old: ScrollPhase, to new: ScrollPhase) -> Bool {
        guard new == .idle else { return false }
        return old == .interacting || old == .decelerating
    }

    /// Whether follow turns off: a scroll by the user ended away from the bottom.
    static func shouldStopFollowing(
        from old: ScrollPhase, to new: ScrollPhase, distanceFromBottom: CGFloat
    ) -> Bool {
        endsUserScroll(from: old, to: new) && !isAtBottom(distance: distanceFromBottom)
    }
}
