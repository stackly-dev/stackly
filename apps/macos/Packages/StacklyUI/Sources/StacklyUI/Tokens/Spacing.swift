import SwiftUI

/// The spacing scale in points. Padding and stacks use these fixed values and never
/// scale them with text size.
nonisolated public enum Spacing {
    /// 2 pt.
    public static let s2: CGFloat = 2
    /// 4 pt.
    public static let s4: CGFloat = 4
    /// 6 pt.
    public static let s6: CGFloat = 6
    /// 8 pt.
    public static let s8: CGFloat = 8
    /// 12 pt.
    public static let s12: CGFloat = 12
    /// 14 pt, the horizontal padding of most mockup rows and cards.
    public static let s14: CGFloat = 14
    /// 16 pt.
    public static let s16: CGFloat = 16
    /// 22 pt, the page inset and the gap between sections.
    public static let s22: CGFloat = 22

    /// The horizontal inset of a page.
    public static let pageHorizontal: CGFloat = s22
    /// The vertical gap between sections of a page.
    public static let sectionGap: CGFloat = s22
    /// The padding inside a custom card: 14 vertical, 16 horizontal.
    public static let cardInsets = EdgeInsets(top: s14, leading: s16, bottom: s14, trailing: s16)
    /// The padding of a row in a grouped list: 11 vertical, 14 horizontal (mockup).
    public static let rowInsets = EdgeInsets(top: 11, leading: s14, bottom: 11, trailing: s14)
    /// The padding of a compact row: 9 vertical, 14 horizontal (mockup).
    public static let compactRowInsets = EdgeInsets(
        top: 9,
        leading: s14,
        bottom: 9,
        trailing: s14
    )
}

/// The corner radius of a custom surface. Native controls, sheets, and dialogs keep the
/// shapes the system draws.
nonisolated public enum Radius: Sendable, CaseIterable {
    /// 8 pt: code blocks and custom table rows.
    case small
    /// 14 pt: grouped lists, cards, and banners (mockup 12 maps here).
    case medium
    /// 16 pt: summary cards, failure banners, and panels (mockup 18 maps here).
    case large

    /// The radius in points.
    public var value: CGFloat {
        switch self {
        case .small: 8
        case .medium: 14
        case .large: 16
        }
    }
}

/// A named default layout size.
nonisolated public struct ScaledSize: Sendable, Hashable {
    /// The size in points at the default text size.
    public let base: CGFloat
    /// The text style the size scales with.
    public let textStyle: Font.TextStyle

    /// The status dot in front of a status label: 7 pt relative to `.body`.
    public static let statusDot = ScaledSize(base: 7, textStyle: .body)
    /// The tone symbol of a banner: 20 pt relative to `.body`.
    public static let bannerSymbol = ScaledSize(base: 20, textStyle: .body)
    /// The minimum height of a tone badge: 20 pt relative to `.subheadline`.
    public static let badgeHeight = ScaledSize(base: 20, textStyle: .subheadline)
    /// The symbol of a page placeholder: 44 pt relative to `.title2`.
    public static let placeholderPageSymbol = ScaledSize(base: 44, textStyle: .title2)
    /// The symbol of a section placeholder: 34 pt relative to `.title3`.
    public static let placeholderSectionSymbol = ScaledSize(base: 34, textStyle: .title3)
    /// The minimum height of a custom row: 34 pt relative to `.body`. Rows never have a
    /// fixed height.
    public static let rowMinHeight = ScaledSize(base: 34, textStyle: .body)
}

/// The smallest custom hit target: 24 × 24 pt. Apply it with
/// `frame(minWidth:minHeight:)` and `contentShape` when a custom control draws smaller.
nonisolated public enum HitTarget {
    /// The minimum width and height in points.
    public static let minimum: CGFloat = 24
}
