import AppKit
import SwiftUI
import Testing

@testable import StacklyUI

/// The Tokens gallery's motion sample keeps the scrollable page still (spec 0012 AC-16).
struct GalleryMotionTests {
    /// AC-16: hiding the banner must not shrink the page and move its bottom scroll position.
    @Test(arguments: [CGFloat(320), 640, 1044])
    func hidingTheBannerPreservesItsLayoutSize(width: CGFloat) {
        let visible = Self.size(isShown: true, width: width)
        let hidden = Self.size(isShown: false, width: width)

        #expect(visible.height > 1)
        #expect(abs(visible.height - hidden.height) < 0.5)
        #expect(abs(visible.width - hidden.width) < 0.5)
    }

    private static func size(isShown: Bool, width: CGFloat) -> CGSize {
        let controller = NSHostingController(
            rootView: GalleryMotionBanner(isShown: isShown)
                .fixedSize(horizontal: false, vertical: true)
        )
        return controller.sizeThatFits(
            in: CGSize(width: width, height: .greatestFiniteMagnitude)
        )
    }
}
