import SwiftUI

/// The only motion in Stackly's own views: an opacity crossfade. There is no animated
/// move, slide, scale, or offset in any setting.
public enum StacklyMotion {
    /// The crossfade animation: 0.2 s, kept while Reduce Motion is on.
    public static let crossfadeAnimation: Animation = .easeInOut(duration: 0.2)

    /// The transition for a status change, a banner appearing, and a change of
    /// `ViewState` shape: `.opacity` with ``crossfadeAnimation``.
    public static let crossfade: AnyTransition = .opacity.animation(crossfadeAnimation)

    /// The animation for a change of a progress value, or `nil` while Reduce Motion is
    /// on so the value jumps. A spinner keeps spinning either way.
    ///
    /// - Parameter reduceMotion: The environment's `accessibilityReduceMotion`.
    public static func progress(reduceMotion: Bool) -> Animation? {
        reduceMotion ? nil : .easeInOut(duration: 0.2)
    }
}

/// Posts a VoiceOver announcement of `text`, once.
///
/// State views call it when a page turns failed or a critical banner appears.
public func announce(_ text: String) {
    AccessibilityNotification.Announcement(text).post()
}
