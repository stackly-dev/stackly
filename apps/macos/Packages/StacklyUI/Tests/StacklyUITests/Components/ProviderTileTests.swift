import AppKit
import StacklyClient
import StacklyTestSupport
import Testing

@testable import StacklyUI

/// The provider tile (spec 0067, AC-2): the default mark, the fill table, and the sizes.
struct ProviderTileTests {
    @Test(arguments: [
        ("PostgreSQL", "Po"),
        ("9front", "9f"),
        ("NATS", "Na"),
        ("valkey", "Va"),
        ("a", "A"),
        ("My SQL", "My"),
        ("-x-y", "Xy"),
        ("Á", "?"),
        ("", "?"),
        ("  ", "?"),
    ])
    func defaultMarkTakesTwoAsciiCharacters(name: String, expected: String) {
        #expect(ProviderMark.defaultMark(for: name) == expected)
    }

    @Test func defaultMarkOfNoNameIsAQuestionMark() {
        #expect(ProviderMark.defaultMark(for: nil) == "?")
    }

    @Test func defaultMarkSkipsNonAsciiCharacters() {
        #expect(ProviderMark.defaultMark(for: "Ábc") == "Bc")
    }

    @Test(
        .evidence(
            covers: ["REQ-024"],
            expected:
                "Spec 0067 AC-2: every tile color maps to its system fill, and nil or unrecognized is gray"
        ))
    func everyTileColorHasItsSystemFill() {
        let table: [(TileColor?, NSColor)] = [
            (.blue, .systemBlue),
            (.cyan, .systemCyan),
            (.teal, .systemTeal),
            (.green, .systemGreen),
            (.yellow, .systemYellow),
            (.orange, .systemOrange),
            (.red, .systemRed),
            (.brown, .systemBrown),
            (.indigo, .systemIndigo),
            (.gray, .systemGray),
            (.unrecognized, .systemGray),
            (nil, .systemGray),
        ]
        for (color, fill) in table {
            #expect(TileFill.color(for: color) == fill, "\(String(describing: color))")
        }
    }

    @Test(
        .evidence(
            covers: ["REQ-024"],
            expected: "Spec 0067 AC-2: the four tile sizes are the mockup sizes"))
    func tileSizesMatchTheMockup() {
        #expect(ProviderTile.smallSize == 18)
        #expect(ProviderTile.rowSize == 26)
        #expect(ProviderTile.mediumSize == 40)
        #expect(ProviderTile.largeSize == 52)
    }

    @Test(
        .evidence(
            covers: ["REQ-024"],
            expected:
                "Spec 0067 AC-2: the catalog mark wins and the default mark of the name fills in"))
    func markTextPrefersTheCatalogMarkAndFallsBackToTheName() {
        let catalog = ProviderTile(mark: "Pg", tile: .blue, name: "Other", size: 40)
        let fallback = ProviderTile(mark: nil, tile: .blue, name: "Valkey", size: 40)
        let empty = ProviderTile(mark: "", tile: .blue, name: "Valkey", size: 40)
        let none = ProviderTile(mark: nil, tile: nil, size: 40)
        #expect(catalog.markText == "Pg")
        #expect(fallback.markText == "Va")
        #expect(empty.markText == "Va")
        #expect(none.markText == "?")
    }

    @Test func tileSpecsCompareByValue() {
        #expect(TileSpec(mark: "Pg", tile: .blue) == TileSpec(mark: "Pg", tile: .blue, name: nil))
        #expect(TileSpec(mark: "Pg", tile: .blue) != TileSpec(mark: "Pg", tile: .red))
    }
}
