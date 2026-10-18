import SwiftUI
import Testing

@testable import StacklyUI

/// The custom color tokens (spec 0012, Color): the table values and the environment
/// lookup.
struct ColorTokenTests {
    /// One row of the spec's custom token table.
    nonisolated struct Row: CustomTestStringConvertible, Sendable {
        let name: String
        let token: ColorToken
        let light: UInt32
        let dark: UInt32
        let highContrastLight: UInt32
        let highContrastDark: UInt32

        var testDescription: String { name }

        func expected(_ appearance: TestAppearance) -> UInt32 {
            switch appearance {
            case .light: light
            case .dark: dark
            case .highContrastLight: highContrastLight
            case .highContrastDark: highContrastDark
            }
        }
    }

    nonisolated static let rows = [
        Row(
            name: "surfaceWindow",
            token: StacklyColor.surfaceWindow,
            light: 0xF5F5F7,
            dark: 0x1E1E20,
            highContrastLight: 0xF5F5F7,
            highContrastDark: 0x1E1E20
        ),
        Row(
            name: "surfaceCard",
            token: StacklyColor.surfaceCard,
            light: 0xFFFFFF,
            dark: 0x2A2A2D,
            highContrastLight: 0xFFFFFF,
            highContrastDark: 0x2A2A2D
        ),
        Row(
            name: "surfaceConsole",
            token: StacklyColor.surfaceConsole,
            light: 0xFFFFFF,
            dark: 0x18181A,
            highContrastLight: 0xFFFFFF,
            highContrastDark: 0x18181A
        ),
        Row(
            name: "textSecondary",
            token: StacklyColor.textSecondary,
            light: 0x606065,
            dark: 0xAEAEB3,
            highContrastLight: 0x4E4E52,
            highContrastDark: 0xC0C0C4
        ),
    ]

    @Test(arguments: rows, TestAppearance.allCases)
    func tokenValueMatchesTheSpecTable(row: Row, appearance: TestAppearance) {
        let color = row.token.value(scheme: appearance.scheme, contrast: appearance.contrast)
        #expect(color.rgb.hex == row.expected(appearance))
    }

    @Test(arguments: rows, [TestAppearance.light, .dark])
    func resolveReadsTheColorSchemeFromTheEnvironment(row: Row, appearance: TestAppearance) {
        let resolved = SRGBColor(row.token.resolve(in: appearance.environment))
        #expect(resolved.hex == row.expected(appearance))
    }

    @Test func surfaceTokensMapToTheirColorTokens() {
        #expect(SurfaceToken.card.color == StacklyColor.surfaceCard)
        #expect(SurfaceToken.window.color == StacklyColor.surfaceWindow)
    }
}
