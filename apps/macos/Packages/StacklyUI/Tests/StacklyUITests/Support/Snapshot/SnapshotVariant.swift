import SwiftUI

@testable import StacklyUI

/// A regular appearance or narrow layout used by a snapshot.
enum SnapshotVariant: CaseIterable, Sendable {
    /// Light at the main size.
    case light
    /// Dark at the main size.
    case dark
    /// Light at the narrow width.
    case narrow

    /// The variants rendered by default.
    static let standard: [SnapshotVariant] = allCases

    /// The filename suffix.
    var suffix: String {
        switch self {
        case .light: "light"
        case .dark: "dark"
        case .narrow: "narrow"
        }
    }

    /// The color scheme to render.
    var appearance: FixtureAppearance {
        switch self {
        case .light, .narrow: .light
        case .dark: .dark
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
