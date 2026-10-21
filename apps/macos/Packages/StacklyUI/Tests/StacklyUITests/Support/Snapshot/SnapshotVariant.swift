import SwiftUI

@testable import StacklyUI

/// One of the images a snapshot renders: a color scheme, Increase Contrast, a narrow
/// width, or Reduce Transparency (spec 0068, AC-5).
enum SnapshotVariant: CaseIterable, Sendable {
    /// Light at the main size.
    case light
    /// Dark at the main size.
    case dark
    /// Light with Increase Contrast.
    case hcLight
    /// Dark with Increase Contrast.
    case hcDark
    /// Light at the narrow width, the large text check of spec 0012.
    case narrow
    /// Light with Reduce Transparency: identical to `light` unless the caller says
    /// `systemMaterial`.
    case reduceTransparency

    /// All six, the default of `assertSnapshot`.
    static let standard: [SnapshotVariant] = allCases

    /// The file suffix: `light`, `dark`, `hc-light`, `hc-dark`, `narrow`, `rt`.
    var suffix: String {
        switch self {
        case .light: "light"
        case .dark: "dark"
        case .hcLight: "hc-light"
        case .hcDark: "hc-dark"
        case .narrow: "narrow"
        case .reduceTransparency: "rt"
        }
    }

    /// The appearance the variant pins.
    var appearance: FixtureAppearance {
        switch self {
        case .light, .narrow: .light
        case .dark: .dark
        case .hcLight: .highContrastLight
        case .hcDark: .highContrastDark
        case .reduceTransparency: .reducedTransparency
        }
    }
}

/// The canvas of a snapshot.
enum SnapshotSize: Sendable, Equatable {
    /// The page area of the default window: 1044 × 748 pt, 640 × 748 pt when narrow.
    case page
    /// The width the page asks for, with its height from `fittingSize`.
    case fitting(width: CGFloat, narrowWidth: CGFloat)

    /// The width of `variant`.
    func width(for variant: SnapshotVariant) -> CGFloat {
        switch self {
        case .page: variant == .narrow ? 640 : 1044
        case .fitting(let width, let narrowWidth): variant == .narrow ? narrowWidth : width
        }
    }

    /// The height of every variant.
    var height: SnapshotHeight {
        switch self {
        case .page: .fixed(748)
        case .fitting: .fitting
        }
    }
}
