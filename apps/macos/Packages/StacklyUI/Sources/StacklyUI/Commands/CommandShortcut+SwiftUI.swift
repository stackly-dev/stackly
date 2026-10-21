import SwiftUI

extension CommandShortcut {
    /// The shortcut as SwiftUI takes it, or `nil` for a key SwiftUI has no name for.
    ///
    /// A letter, digit, or punctuation key goes over as it is. Punctuation that needs ⇧ goes
    /// over as the shifted character without ⇧, so the menu shows "⌘?" and not "⇧⌘/".
    var keyboardShortcut: KeyboardShortcut? {
        var modifiers = EventModifiers()
        if self.modifiers.contains(.command) { modifiers.insert(.command) }
        if self.modifiers.contains(.shift) { modifiers.insert(.shift) }
        if self.modifiers.contains(.option) { modifiers.insert(.option) }
        if self.modifiers.contains(.control) { modifiers.insert(.control) }
        if let shifted = Self.shiftedCharacter(ofBaseKey: key), modifiers.contains(.shift) {
            modifiers.remove(.shift)
            return KeyboardShortcut(KeyEquivalent(shifted), modifiers: modifiers)
        }
        let named: [String: KeyEquivalent] = [
            "tab": .tab, "space": .space, "escape": .escape, "return": .return,
            "delete": .delete, "forwardDelete": .deleteForward, "up": .upArrow,
            "down": .downArrow, "left": .leftArrow, "right": .rightArrow,
        ]
        if let equivalent = named[key] {
            return KeyboardShortcut(equivalent, modifiers: modifiers)
        }
        guard key.count == 1, let character = key.first else { return nil }
        return KeyboardShortcut(KeyEquivalent(character), modifiers: modifiers)
    }
}

extension CommandEntry {
    /// The shortcut of the row as SwiftUI takes it, or `nil` when the row has none.
    var keyboardShortcut: KeyboardShortcut? {
        shortcut?.keyboardShortcut
    }
}
