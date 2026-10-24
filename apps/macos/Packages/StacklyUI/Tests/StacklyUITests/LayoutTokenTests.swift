import SwiftUI
import Testing

@testable import StacklyUI

/// Spacing, radius, scaled sizes, and motion (spec 0012, Spacing, radius, and size;
/// Motion).
struct LayoutTokenTests {
    @Test func spacingScaleMatchesTheSpec() {
        let scale = [
            Spacing.s2, Spacing.s4, Spacing.s6, Spacing.s8,
            Spacing.s12, Spacing.s14, Spacing.s16, Spacing.s22,
        ]
        #expect(scale == [2, 4, 6, 8, 12, 14, 16, 22])
    }

    @Test func namedInsetsMatchTheSpec() {
        #expect(Spacing.pageHorizontal == 22)
        #expect(Spacing.sectionGap == 22)
        #expect(Spacing.cardInsets == EdgeInsets(top: 14, leading: 16, bottom: 14, trailing: 16))
        #expect(Spacing.rowInsets == EdgeInsets(top: 11, leading: 14, bottom: 11, trailing: 14))
        #expect(
            Spacing.compactRowInsets == EdgeInsets(top: 9, leading: 14, bottom: 9, trailing: 14)
        )
    }

    @Test func radiusValuesMatchTheSpec() {
        #expect(Radius.allCases.map(\.value) == [8, 14, 16])
    }

    @Test func scaledSizeDefaultsMatchTheSpec() {
        #expect(ScaledSize.statusDot == ScaledSize(base: 7, textStyle: .body))
        #expect(ScaledSize.bannerSymbol == ScaledSize(base: 20, textStyle: .body))
        #expect(ScaledSize.badgeHeight == ScaledSize(base: 20, textStyle: .subheadline))
        #expect(ScaledSize.placeholderPageSymbol == ScaledSize(base: 44, textStyle: .title2))
        #expect(ScaledSize.placeholderSectionSymbol == ScaledSize(base: 34, textStyle: .title3))
        #expect(ScaledSize.rowMinHeight == ScaledSize(base: 34, textStyle: .body))
    }

    @Test func customHitTargetsAreAtLeastTwentyFourPoints() {
        #expect(HitTarget.minimum == 24)
    }

}
