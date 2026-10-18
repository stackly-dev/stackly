import AppKit
import SwiftUI
import Testing

@testable import StacklyUI

/// The four appearances the contrast table is checked in: light and dark, each with
/// and without Increase Contrast.
nonisolated enum TestAppearance: CaseIterable, CustomTestStringConvertible, Sendable {
    case light
    case dark
    case highContrastLight
    case highContrastDark

    /// The SwiftUI color scheme of the appearance.
    var scheme: ColorScheme {
        switch self {
        case .light, .highContrastLight: .light
        case .dark, .highContrastDark: .dark
        }
    }

    /// The SwiftUI color scheme contrast of the appearance.
    var contrast: ColorSchemeContrast {
        isHighContrast ? .increased : .standard
    }

    /// Whether Increase Contrast is on.
    var isHighContrast: Bool {
        switch self {
        case .light, .dark: false
        case .highContrastLight, .highContrastDark: true
        }
    }

    /// The AppKit appearance that resolves system colors for this appearance.
    var appearanceName: NSAppearance.Name {
        switch self {
        case .light: .aqua
        case .dark: .darkAqua
        case .highContrastLight: .accessibilityHighContrastAqua
        case .highContrastDark: .accessibilityHighContrastDarkAqua
        }
    }

    /// The minimum contrast for text on a plain surface: 4.5:1, or 7:1 under Increase
    /// Contrast (spec 0012, contrast pairs P1 and P2).
    var plainSurfaceMinimum: Double {
        isHighContrast ? 7 : 4.5
    }

    /// The SwiftUI environment for this appearance. `colorSchemeContrast` cannot be set
    /// on `EnvironmentValues`, so only light and dark resolve correctly through it.
    var environment: EnvironmentValues {
        var environment = EnvironmentValues()
        environment.colorScheme = scheme
        return environment
    }

    /// Resolves a system color for this appearance into sRGB with its alpha.
    func resolve(_ color: NSColor) throws -> RGBA {
        let appearance = try #require(NSAppearance(named: appearanceName))
        var resolved: NSColor?
        appearance.performAsCurrentDrawingAppearance {
            resolved = color.usingColorSpace(.sRGB)
        }
        let srgb = try #require(resolved)
        return RGBA(
            SRGBColor(
                red: Double(srgb.redComponent),
                green: Double(srgb.greenComponent),
                blue: Double(srgb.blueComponent)
            ),
            alpha: Double(srgb.alphaComponent)
        )
    }

    /// The custom token's value for this appearance.
    func resolve(_ token: ColorToken) -> SRGBColor {
        token.components(scheme: scheme, contrast: contrast)
    }

    var testDescription: String {
        switch self {
        case .light: "light"
        case .dark: "dark"
        case .highContrastLight: "high contrast light"
        case .highContrastDark: "high contrast dark"
        }
    }
}
