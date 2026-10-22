import AppKit
import StacklyClient
import Testing

@testable import StacklyUI

/// The catalog status tables of spec 0017 (AC-12, *Tone dan label*) and the words of the
/// foreign values (*Nilai asing*).
struct CatalogAppearanceTests {
    struct Row: CustomTestStringConvertible, Sendable {
        let name: String
        let appearance: StatusAppearance
        let label: String
        let tone: Tone
        let symbol: String
        var testDescription: String { name }
    }

    nonisolated static let supportRows: [Row] = [
        Row(
            name: "supported",
            appearance: CatalogAppearance.appearance(of: SupportStatus.supported),
            label: "Tested", tone: .positive, symbol: "checkmark.circle.fill"),
        Row(
            name: "experimental",
            appearance: CatalogAppearance.appearance(of: SupportStatus.experimental),
            label: "Experimental", tone: .caution, symbol: "exclamationmark.triangle.fill"),
        Row(
            name: "unverified",
            appearance: CatalogAppearance.appearance(of: SupportStatus.unverified),
            label: "Unverified", tone: .neutral, symbol: "questionmark.circle.fill"),
        Row(
            name: "planned", appearance: CatalogAppearance.appearance(of: SupportStatus.planned),
            label: "Planned", tone: .neutral, symbol: "clock.fill"),
        Row(
            name: "unsupported",
            appearance: CatalogAppearance.appearance(of: SupportStatus.unsupported),
            label: "Unsupported", tone: .neutral, symbol: "minus.circle.fill"),
        Row(
            name: "blocked", appearance: CatalogAppearance.appearance(of: SupportStatus.blocked),
            label: "Blocked", tone: .caution, symbol: "lock.circle.fill"),
        Row(
            name: "unrecognized",
            appearance: CatalogAppearance.appearance(of: SupportStatus.unrecognized),
            label: "Unrecognized State", tone: .caution, symbol: "questionmark.diamond.fill"),
    ]

    nonisolated static let stateRows: [Row] = [
        Row(
            name: "available",
            appearance: CatalogAppearance.appearance(of: CatalogEntryState.available),
            label: "Available", tone: .neutral, symbol: "arrow.down.circle.fill"),
        Row(
            name: "blocked",
            appearance: CatalogAppearance.appearance(of: CatalogEntryState.blocked),
            label: "Blocked", tone: .caution, symbol: "lock.circle.fill"),
        Row(
            name: "unsupported",
            appearance: CatalogAppearance.appearance(of: CatalogEntryState.unsupported),
            label: "Unsupported", tone: .neutral, symbol: "minus.circle.fill"),
        Row(
            name: "planned",
            appearance: CatalogAppearance.appearance(of: CatalogEntryState.planned),
            label: "Planned", tone: .neutral, symbol: "clock.fill"),
        Row(
            name: "invalid",
            appearance: CatalogAppearance.appearance(of: CatalogEntryState.invalid),
            label: "Invalid", tone: .critical, symbol: "xmark.circle.fill"),
        Row(
            name: "unrecognized",
            appearance: CatalogAppearance.appearance(of: CatalogEntryState.unrecognized),
            label: "Unrecognized State", tone: .caution, symbol: "questionmark.diamond.fill"),
    ]

    @Test(arguments: supportRows)
    func eachSupportStatusHasItsLabelToneAndSymbol(row: Row) {
        #expect(row.appearance.label == row.label)
        #expect(row.appearance.tone == row.tone)
        #expect(row.appearance.symbol == row.symbol)
    }

    @Test(arguments: stateRows)
    func eachEntryStateHasItsLabelToneAndSymbol(row: Row) {
        #expect(row.appearance.label == row.label)
        #expect(row.appearance.tone == row.tone)
        #expect(row.appearance.symbol == row.symbol)
    }

    @Test func anUnrecognizedStateSaysToUpdateStackly() {
        for appearance in [
            CatalogAppearance.appearance(of: SupportStatus.unrecognized),
            CatalogAppearance.appearance(of: CatalogEntryState.unrecognized),
        ] {
            #expect(appearance.detail?.text == "Update Stackly to see this state")
            #expect(appearance.detail?.tone == .caution)
        }
    }

    @Test func everySymbolExistsOnThisMac() {
        for row in Self.supportRows + Self.stateRows {
            #expect(
                NSImage(systemSymbolName: row.symbol, accessibilityDescription: nil) != nil,
                "\(row.symbol) is not an SF Symbol")
        }
    }

    @Test func statesOfOneToneKeepTheirOwnSymbolShape() {
        let neutral = Self.supportRows.filter { $0.tone == .neutral }
        #expect(Set(neutral.map(\.symbol)).count == neutral.count)
        let caution = Self.supportRows.filter { $0.tone == .caution }
        #expect(Set(caution.map(\.symbol)).count == caution.count)
    }

    @Test func nothingMarksUnsupportedOrBlockedAsAFailure() {
        for row in Self.supportRows + Self.stateRows
        where ["unsupported", "blocked", "planned", "available"].contains(row.name) {
            #expect(row.tone != .critical)
        }
    }

    @Test func theMappingHasNoDefaultCase() throws {
        let file = try SourceTree.swiftFiles().first {
            $0.path.hasSuffix("Catalog/CatalogAppearance.swift")
        }
        let source = try #require(file)
        #expect(source.codeLines.contains { $0.contains("default:") } == false)
    }
}
