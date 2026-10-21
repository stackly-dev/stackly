import SwiftUI
import Testing

@testable import StacklyUI

/// The divider rule and the SwiftUI shortcut of a menu item (spec 0069, AC-1, AC-3).
struct CommandItemTests {
    private func row(_ id: CommandID, group: Int) -> CommandEntry {
        CommandEntry(
            id, menu: .view, group: group, title: "T", scope: .shell, owner: 69,
            status: .built)
    }

    @Test func aDividerNeedsABuiltGroupBeforeAndTheGroupItself() {
        let layout = [[row(.showOverview, group: 1)], [row(.back, group: 2)]]
        #expect(GroupDivider.shows(layout, before: 2))
        #expect(GroupDivider.shows(layout, before: 1) == false)
    }

    @Test func noDividerForAGroupThatHasNoBuiltRow() {
        let layout = [[row(.showOverview, group: 1)], [row(.toggleInspector, group: 3)]]
        #expect(GroupDivider.shows(layout, before: 2) == false)
        #expect(GroupDivider.shows(layout, before: 3))
    }

    @Test func noDividerInAnEmptyLayout() {
        #expect(GroupDivider.shows([], before: 1) == false)
        #expect(GroupDivider.shows([], before: 2) == false)
    }

    @Test func aLetterShortcutGoesToSwiftUIAsItIs() throws {
        let shortcut = try #require(CommandShortcut("r", [.command, .shift]).keyboardShortcut)
        #expect(shortcut.key == KeyEquivalent("r"))
        #expect(shortcut.modifiers == [.command, .shift])
    }

    @Test func shiftedPunctuationGoesAsTheShiftedCharacterWithoutShift() throws {
        let shortcut = try #require(CommandShortcut("?", .command).keyboardShortcut)
        #expect(shortcut.key == KeyEquivalent("?"))
        #expect(shortcut.modifiers == .command)
    }

    @Test func unshiftedPunctuationKeepsItsKey() throws {
        let shortcut = try #require(CommandShortcut("[", .command).keyboardShortcut)
        #expect(shortcut.key == KeyEquivalent("["))
        #expect(shortcut.modifiers == .command)
    }

    @Test func everyShortcutInTheMapConverts() {
        for entry in CommandMap.entries where entry.shortcut != nil {
            #expect(entry.keyboardShortcut != nil, "\(entry.id)")
        }
    }

    @Test func aRowWithoutAShortcutHasNoKeyboardShortcut() {
        #expect(CommandMap.entry(for: .reconnect).keyboardShortcut == nil)
    }
}
