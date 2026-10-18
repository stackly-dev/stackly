import AppKit
import SwiftUI
import Testing

@testable import StacklyUI

/// The contrast pairs of spec 0012 (AC-3), computed from the token table and from
/// system colors resolved per `NSAppearance`, in all four appearances.
struct ContrastTests {
    /// A text color, either a system color or a custom token.
    enum TextColor: CustomStringConvertible {
        case system(String, NSColor)
        case token(String, ColorToken)

        var description: String {
            switch self {
            case .system(let name, _), .token(let name, _): name
            }
        }

        func resolve(in appearance: TestAppearance) throws -> RGBA {
            switch self {
            case .system(_, let color): try appearance.resolve(color)
            case .token(_, let token): RGBA(appearance.resolve(token))
            }
        }
    }

    /// The plain surfaces of pair P1, resolved for one appearance.
    static func plainSurfaces(in appearance: TestAppearance) throws -> [(String, SRGBColor)] {
        [
            ("surfaceWindow", appearance.resolve(StacklyColor.surfaceWindow)),
            ("surfaceCard", appearance.resolve(StacklyColor.surfaceCard)),
            ("surfaceConsole", appearance.resolve(StacklyColor.surfaceConsole)),
            ("windowBackgroundColor", try appearance.resolve(.windowBackgroundColor).rgb),
            ("controlBackgroundColor", try appearance.resolve(.controlBackgroundColor).rgb),
        ]
    }

    /// The primary and secondary text of pairs P1 and P4.
    static let neutralText: [TextColor] = [
        .system("labelColor", .labelColor),
        .token("textSecondary", StacklyColor.textSecondary),
    ]

    /// The text of each tone, for pairs P2 and P3.
    static let toneText: [TextColor] = Tone.allCases.map { .token("\($0) text", $0.text) }

    /// A tone surface resolved for one appearance, mixed the same way the view mixes it.
    static func toneSurface(
        _ tone: Tone,
        on base: SurfaceToken,
        strength: ToneStrength,
        in appearance: TestAppearance
    ) throws -> SRGBColor {
        let fill = try appearance.resolve(tone.systemFill).rgb
        return appearance.resolve(base.color).mixed(with: fill, fraction: strength.fraction)
    }

    @Test(arguments: TestAppearance.allCases)
    func p1PrimaryAndSecondaryTextPassOnEveryPlainSurface(appearance: TestAppearance) throws {
        for (surfaceName, surface) in try Self.plainSurfaces(in: appearance) {
            for text in Self.neutralText {
                let ratio = WCAG.contrast(text: try text.resolve(in: appearance), on: surface)
                #expect(
                    ratio >= appearance.plainSurfaceMinimum,
                    "\(text) on \(surfaceName): \(ratio)"
                )
            }
        }
    }

    @Test(arguments: TestAppearance.allCases)
    func p2ToneTextPassesOnEveryPlainSurface(appearance: TestAppearance) throws {
        for (surfaceName, surface) in try Self.plainSurfaces(in: appearance) {
            for text in Self.toneText {
                let ratio = WCAG.contrast(text: try text.resolve(in: appearance), on: surface)
                #expect(
                    ratio >= appearance.plainSurfaceMinimum,
                    "\(text) on \(surfaceName): \(ratio)"
                )
            }
        }
    }

    @Test(arguments: TestAppearance.allCases)
    func p3ToneTextPassesOnItsOwnTint(appearance: TestAppearance) throws {
        for tone in Tone.allCases {
            let text = RGBA(appearance.resolve(tone.text))
            for base in SurfaceToken.allCases {
                for strength in ToneStrength.allCases {
                    let surface = try Self.toneSurface(
                        tone,
                        on: base,
                        strength: strength,
                        in: appearance
                    )
                    let ratio = WCAG.contrast(text: text, on: surface)
                    #expect(
                        ratio >= 4.5,
                        "\(tone) text on \(tone) \(strength) over \(base): \(ratio)"
                    )
                }
            }
        }
    }

    @Test(arguments: TestAppearance.allCases)
    func p4PrimaryAndSecondaryTextPassOnEveryBannerTint(appearance: TestAppearance) throws {
        for tone in Tone.allCases {
            for base in SurfaceToken.allCases {
                let surface = try Self.toneSurface(
                    tone,
                    on: base,
                    strength: .banner,
                    in: appearance
                )
                for text in Self.neutralText {
                    let ratio = WCAG.contrast(text: try text.resolve(in: appearance), on: surface)
                    #expect(ratio >= 4.5, "\(text) on \(tone) banner over \(base): \(ratio)")
                }
            }
        }
    }

    @Test func contrastMathMatchesKnownWCAGValues() {
        let black = SRGBColor(hex: 0x000000)
        let white = SRGBColor(hex: 0xFFFFFF)
        #expect(abs(WCAG.contrast(black, white) - 21) < 0.001)
        #expect(abs(WCAG.contrast(SRGBColor(hex: 0x767676), white) - 4.54) < 0.01)
        #expect(WCAG.contrast(text: RGBA(black, alpha: 0), on: white) == 1)
    }
}
