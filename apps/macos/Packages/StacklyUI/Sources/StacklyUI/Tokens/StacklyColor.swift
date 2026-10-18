import AppKit
import SwiftUI

// The token file: the only place in Stackly that may name the system secondary
// and tertiary label colors. The source rule test (spec 0012 AC-3) allows them here.

/// A color in the sRGB color space with gamma encoded components from 0 to 1.
nonisolated struct SRGBColor: Sendable, Hashable {
    /// The red component.
    var red: Double
    /// The green component.
    var green: Double
    /// The blue component.
    var blue: Double

    /// Creates a color from gamma encoded sRGB components.
    init(red: Double, green: Double, blue: Double) {
        self.red = red
        self.green = green
        self.blue = blue
    }

    /// Creates a color from a `0xRRGGBB` value.
    init(hex: UInt32) {
        red = Double((hex >> 16) & 0xFF) / 255
        green = Double((hex >> 8) & 0xFF) / 255
        blue = Double(hex & 0xFF) / 255
    }

    /// Creates a color from a resolved SwiftUI color, clamped to the sRGB gamut.
    init(_ resolved: Color.Resolved) {
        red = min(max(Double(resolved.red), 0), 1)
        green = min(max(Double(resolved.green), 0), 1)
        blue = min(max(Double(resolved.blue), 0), 1)
    }

    /// The SwiftUI color with these components.
    var color: Color {
        Color(.sRGB, red: red, green: green, blue: blue)
    }

    /// The resolved SwiftUI color with these components.
    var resolved: Color.Resolved {
        Color.Resolved(colorSpace: .sRGB, red: Float(red), green: Float(green), blue: Float(blue))
    }

    /// Mixes `fill` into this color, like CSS `color-mix(in srgb, ...)` in the mockup:
    /// `fill × fraction + self × (1 − fraction)` on the gamma encoded components.
    func mixed(with fill: SRGBColor, fraction: Double) -> SRGBColor {
        SRGBColor(
            red: fill.red * fraction + red * (1 - fraction),
            green: fill.green * fraction + green * (1 - fraction),
            blue: fill.blue * fraction + blue * (1 - fraction)
        )
    }
}

/// A custom Stackly color with one sRGB value per appearance: light, dark, and the
/// high contrast light and dark values used while Increase Contrast is on.
///
/// Use it as a `ShapeStyle`, for example `.foregroundStyle(StacklyColor.textSecondary)`.
/// It reads `colorScheme` and `colorSchemeContrast` from the environment, so text,
/// backgrounds, and borders always pick the same variant, also when the Appearance
/// setting forces a scheme with `preferredColorScheme`. There is no `NSColor`
/// accessor and no asset catalog entry.
nonisolated public struct ColorToken: ShapeStyle, Hashable, Sendable {
    let light: SRGBColor
    let dark: SRGBColor
    let highContrastLight: SRGBColor
    let highContrastDark: SRGBColor

    init(light: UInt32, dark: UInt32, highContrastLight: UInt32, highContrastDark: UInt32) {
        self.light = SRGBColor(hex: light)
        self.dark = SRGBColor(hex: dark)
        self.highContrastLight = SRGBColor(hex: highContrastLight)
        self.highContrastDark = SRGBColor(hex: highContrastDark)
    }

    /// The sRGB components for one appearance.
    func components(scheme: ColorScheme, contrast: ColorSchemeContrast) -> SRGBColor {
        switch (scheme, contrast) {
        case (.dark, .increased): highContrastDark
        case (.dark, _): dark
        case (_, .increased): highContrastLight
        default: light
        }
    }

    /// The color for one appearance, straight from the token table.
    public func value(scheme: ColorScheme, contrast: ColorSchemeContrast) -> Color {
        components(scheme: scheme, contrast: contrast).color
    }

    /// Resolves the token for the environment's color scheme and contrast.
    public func resolve(in environment: EnvironmentValues) -> Color.Resolved {
        components(scheme: environment.colorScheme, contrast: environment.colorSchemeContrast)
            .resolved
    }
}

/// The custom colors of Stackly, from the mockup tokens named on each member.
///
/// Every other neutral comes from the system: `labelColor` for primary text (SwiftUI
/// `.primary`), `separatorColor`, `windowBackgroundColor`, `controlBackgroundColor`, and
/// the backgrounds of native controls. Code never uses the system secondary or tertiary
/// label colors directly; it uses ``textSecondary`` and ``decorative`` instead.
nonisolated public enum StacklyColor {
    /// The content background of pages built from custom cards, such as Overview,
    /// Summary, and Diagnostics (mockup `--win`). Pages built from a native `Table`,
    /// `List`, or `Form` keep the system background.
    public static let surfaceWindow = ColorToken(
        light: 0xF5F5F7,
        dark: 0x1E1E20,
        highContrastLight: 0xF5F5F7,
        highContrastDark: 0x1E1E20
    )

    /// The background of custom cards, drawn by `cardSurface(radius:)` (mockup `--card`).
    public static let surfaceCard = ColorToken(
        light: 0xFFFFFF,
        dark: 0x2A2A2D,
        highContrastLight: 0xFFFFFF,
        highContrastDark: 0x2A2A2D
    )

    /// The background of code blocks and log views (mockup `--console`).
    public static let surfaceConsole = ColorToken(
        light: 0xFFFFFF,
        dark: 0x18181A,
        highContrastLight: 0xFFFFFF,
        highContrastDark: 0x18181A
    )

    /// Secondary text, AA on every surface and 7:1 under Increase Contrast (mockup
    /// `--text2`). Also the text color of the `neutral` tone, and the style to hand to
    /// native APIs that accept one for their secondary text.
    public static let textSecondary = ColorToken(
        light: 0x606065,
        dark: 0xAEAEB3,
        highContrastLight: 0x4E4E52,
        highContrastDark: 0xC0C0C4
    )

    /// Disabled text and decorative icons only, such as placeholder symbols and
    /// chevrons: the system tertiary label color. Never for text that carries meaning.
    public static let decorative = Color(nsColor: .tertiaryLabelColor)
}

/// The opaque base a tone surface is mixed onto.
nonisolated public enum SurfaceToken: Sendable, CaseIterable {
    /// A custom card, ``StacklyColor/surfaceCard``.
    case card
    /// The page background, ``StacklyColor/surfaceWindow``.
    case window

    /// The color token of the base.
    var color: ColorToken {
        switch self {
        case .card: StacklyColor.surfaceCard
        case .window: StacklyColor.surfaceWindow
        }
    }
}
