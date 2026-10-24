import AppKit
import SwiftUI
import Testing

@testable import StacklyUI

/// The four appearances the contrast table is checked in: light and dark, each with
/// and without Increase Contrast.
nonisolated enum TestAppearance: CaseIterable, CustomTestStringConvertible, Sendable {
    case light
    case dark

    /// The SwiftUI color scheme of the appearance.
    var scheme: ColorScheme {
        switch self {
        case .light: .light
        case .dark: .dark
        }
    }

    /// The AppKit appearance that resolves system colors for this appearance.
    var appearanceName: NSAppearance.Name {
        switch self {
        case .light: .aqua
        case .dark: .darkAqua
        }
    }

    /// The SwiftUI environment for this appearance: the color scheme and, through the
    /// underscored setter that tests may use (spec 0068), the contrast.
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
        token.components(scheme: scheme)
    }

    var testDescription: String {
        switch self {
        case .light: "light"
        case .dark: "dark"
        }
    }
}
