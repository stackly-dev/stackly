import AppKit
import SwiftUI
import Testing

@testable import StacklyUI

/// The five tones and their surfaces (spec 0012, Tone and status).
struct ToneTests {
    /// One row of the spec's tone table.
    nonisolated struct Row: CustomTestStringConvertible, Sendable {
        let tone: Tone
        let fill: NSColor
        let text: [TestAppearance: UInt32]
        let symbol: String

        var testDescription: String { "\(tone)" }
    }

    nonisolated static let rows = [
        Row(
            tone: .positive,
            fill: .systemGreen,
            text: [
                .light: 0x18712D, .dark: 0x4ADE74,
            ],
            symbol: "checkmark.circle.fill"
        ),
        Row(
            tone: .caution,
            fill: .systemOrange,
            text: [
                .light: 0x965000, .dark: 0xFFB340,
            ],
            symbol: "exclamationmark.triangle.fill"
        ),
        Row(
            tone: .critical,
            fill: .systemRed,
            text: [
                .light: 0xB8160C, .dark: 0xFF867F,
            ],
            symbol: "xmark.circle.fill"
        ),
        Row(
            tone: .neutral,
            fill: .systemGray,
            text: [
                .light: 0x606065, .dark: 0xAEAEB3,
            ],
            symbol: "info.circle.fill"
        ),
        Row(
            tone: .informative,
            fill: .systemBlue,
            text: [
                .light: 0x0759BA, .dark: 0x62AEFF,
            ],
            symbol: "ellipsis.circle.fill"
        ),
    ]

    @Test func everyToneHasOneRowInTheTable() {
        #expect(Self.rows.map(\.tone) == Tone.allCases)
    }

    @Test(arguments: rows, TestAppearance.allCases)
    func toneTextMatchesTheSpecTable(row: Row, appearance: TestAppearance) {
        #expect(appearance.resolve(row.tone.text).hex == row.text[appearance])
    }

    @Test(arguments: rows)
    func toneFillIsTheSystemColor(row: Row) {
        #expect(row.tone.systemFill == row.fill)
    }

    @Test(arguments: rows)
    func toneSymbolExistsInSFSymbols(row: Row) {
        #expect(row.tone.symbol == row.symbol)
        #expect(NSImage(systemSymbolName: row.symbol, accessibilityDescription: nil) != nil)
    }

    @Test func neutralTextIsTextSecondary() {
        #expect(Tone.neutral.text == StacklyColor.textSecondary)
    }

    @Test func badgeTintsSixteenPercentAndBannerTwelvePercent() {
        #expect(ToneStrength.badge.fraction == 0.16)
        #expect(ToneStrength.banner.fraction == 0.12)
    }

    @Test(
        arguments: Tone.allCases,
        [TestAppearance.light, .dark]
    )
    func toneSurfaceMixesTheResolvedFillIntoItsBase(
        tone: Tone,
        appearance: TestAppearance
    ) throws {
        let fill = try appearance.resolve(tone.systemFill).rgb
        for base in SurfaceToken.allCases {
            for strength in ToneStrength.allCases {
                let style = ToneSurfaceStyle(tone: tone, base: base, strength: strength)
                let resolved = SRGBColor(style.resolve(in: appearance.environment))
                let expected = appearance.resolve(base.color)
                    .mixed(with: fill, fraction: strength.fraction)
                #expect(
                    resolved.isClose(to: expected),
                    "\(base) \(strength): \(resolved.hexString) is not \(expected.hexString)"
                )
            }
        }
    }

    @Test func mixingIsLinearInGammaEncodedSRGB() {
        let base = SRGBColor(hex: 0xFFFFFF)
        let fill = SRGBColor(hex: 0x000000)
        #expect(base.mixed(with: fill, fraction: 0.25).isClose(to: SRGBColor(hex: 0xBFBFBF)))
    }
}
