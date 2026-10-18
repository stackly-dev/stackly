import Foundation
import SwiftUI

@testable import StacklyUI

/// A gamma encoded sRGB color with straight alpha, as a system color resolves.
nonisolated struct RGBA: Sendable {
    /// The color components.
    var rgb: SRGBColor
    /// The alpha, from 0 to 1.
    var alpha: Double

    /// An opaque color.
    init(_ rgb: SRGBColor) {
        self.rgb = rgb
        alpha = 1
    }

    /// A color with alpha.
    init(_ rgb: SRGBColor, alpha: Double) {
        self.rgb = rgb
        self.alpha = alpha
    }

    /// The opaque color this one gives when drawn over `background`.
    func composited(over background: SRGBColor) -> SRGBColor {
        background.mixed(with: rgb, fraction: alpha)
    }
}

/// WCAG 2 contrast math.
nonisolated enum WCAG {
    /// The relative luminance of a gamma encoded sRGB color.
    static func luminance(_ color: SRGBColor) -> Double {
        func linear(_ value: Double) -> Double {
            value <= 0.04045 ? value / 12.92 : pow((value + 0.055) / 1.055, 2.4)
        }
        return 0.2126 * linear(color.red) + 0.7152 * linear(color.green)
            + 0.0722 * linear(color.blue)
    }

    /// The contrast ratio of two opaque colors, from 1 to 21.
    static func contrast(_ first: SRGBColor, _ second: SRGBColor) -> Double {
        let lighter = max(luminance(first), luminance(second))
        let darker = min(luminance(first), luminance(second))
        return (lighter + 0.05) / (darker + 0.05)
    }

    /// The contrast of text, composited first when it has alpha, on an opaque surface.
    static func contrast(text: RGBA, on surface: SRGBColor) -> Double {
        contrast(text.composited(over: surface), surface)
    }
}

nonisolated extension SRGBColor {
    /// The color as `0xRRGGBB`, each component rounded to 8 bits.
    var hex: UInt32 {
        func byte(_ value: Double) -> UInt32 {
            UInt32((min(max(value, 0), 1) * 255).rounded())
        }
        return byte(red) << 16 | byte(green) << 8 | byte(blue)
    }

    /// The color as `#RRGGBB`, for messages.
    var hexString: String {
        String(format: "#%06X", hex)
    }

    /// Whether every component is within `tolerance` of `other`'s.
    func isClose(to other: SRGBColor, tolerance: Double = 1.0 / 255) -> Bool {
        abs(red - other.red) <= tolerance && abs(green - other.green) <= tolerance
            && abs(blue - other.blue) <= tolerance
    }
}

extension Color {
    /// The color's gamma encoded sRGB components, resolved in a default environment.
    var rgb: SRGBColor {
        SRGBColor(resolve(in: EnvironmentValues()))
    }
}
