import SwiftUI
import Testing

/// The text styles behind the typography table (spec 0012, Typography).
struct TypographyTests {
    /// The macOS size column of the typography table, in points.
    static let sizes: [(Font, String, CGFloat)] = [
        (.largeTitle, "largeTitle", 26),
        (.title, "title", 22),
        (.title2, "title2", 17),
        (.title3, "title3", 15),
        (.headline, "headline", 13),
        (.body, "body", 13),
        (.callout, "callout", 12),
        (.subheadline, "subheadline", 11),
        (.footnote, "footnote", 10),
    ]

    @Test func textStylesHaveTheMacOSSizesOfTheTable() {
        let context = EnvironmentValues().fontResolutionContext
        for (font, name, size) in Self.sizes {
            #expect(font.resolve(in: context).pointSize == size, "\(name)")
        }
    }

    /// Step 4 of the build plan: on macOS 27, text styles keep their size at every
    /// `dynamicTypeSize`, so large text is checked at a narrow width instead (see
    /// `apps/macos/design.md`, Accessibility). If this fails, macOS started scaling text
    /// styles: switch the gallery's third preview back to `.dynamicTypeSize(.accessibility3)`
    /// and update design.md.
    @Test(arguments: [DynamicTypeSize.xSmall, .xxxLarge, .accessibility3, .accessibility5])
    func textStylesIgnoreDynamicTypeSizeOnMacOS(size: DynamicTypeSize) {
        var environment = EnvironmentValues()
        let standard = Font.body.resolve(in: environment.fontResolutionContext).pointSize
        environment.dynamicTypeSize = size
        let scaled = Font.body.resolve(in: environment.fontResolutionContext).pointSize
        #expect(scaled == standard)
    }
}
