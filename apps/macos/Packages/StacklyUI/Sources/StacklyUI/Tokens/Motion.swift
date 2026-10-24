import SwiftUI

/// The only motion in Stackly's own views: an opacity crossfade. There is no animated
/// move, slide, scale, or offset in any setting.
public enum StacklyMotion {
    /// The opacity crossfade animation, lasting 0.2 seconds.
    public static let crossfadeAnimation: Animation = .easeInOut(duration: 0.2)

    /// The transition for a status change, a banner appearing, and a change of
    /// `ViewState` shape: `.opacity` with ``crossfadeAnimation``.
    public static let crossfade: AnyTransition = .opacity.animation(crossfadeAnimation)

    /// A spinner keeps spinning either way.
    public static func progress() -> Animation? {
        .easeInOut(duration: 0.2)
    }
}
