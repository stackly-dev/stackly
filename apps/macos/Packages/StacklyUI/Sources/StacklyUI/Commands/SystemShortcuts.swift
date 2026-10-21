import Foundation

/// Where the knowledge of a macOS shortcut comes from.
nonisolated public enum SystemShortcutSource: String, Hashable, Sendable {
    /// The shortcut table of the Apple Human Interface Guidelines, Keyboards.
    case hig
    /// The Apple Support list "Mac keyboard shortcuts".
    case macOS
    /// The standard AppKit key bindings of a text field.
    case appKit
    /// A command SwiftUI adds on its own, such as `SidebarCommands`.
    case swiftUI
    /// An item the macOS 27 menu probe found in the menu bar (spec 0069, step 1).
    case probe
}

/// One shortcut macOS or AppKit already uses.
nonisolated public struct SystemShortcut: Hashable, Sendable {
    /// The shortcut.
    public let shortcut: CommandShortcut
    /// What it does.
    public let title: String
    /// Where the entry comes from.
    public let source: SystemShortcutSource
    /// The standard meaning, for a shortcut a row may reuse for the same job; `nil` for
    /// the other tables.
    public let meaning: StandardMeaning?

    /// Creates an entry.
    init(
        _ key: String, _ modifiers: CommandModifiers, _ title: String,
        _ source: SystemShortcutSource, meaning: StandardMeaning? = nil
    ) {
        self.shortcut = CommandShortcut(key, modifiers)
        self.title = title
        self.source = source
        self.meaning = meaning
    }
}

/// The shortcuts Stackly may not take, or may take only for the same job (spec 0069,
/// tables A to C and rules D and E).
///
/// The tables were written from the Apple documents and then compared with the real menu
/// bar of macOS 27 by the probe of step 1; `design.md` lists what the probe added.
nonisolated public enum SystemShortcuts {
    private static let cmd = CommandModifiers.command
    private static let shift = CommandModifiers.shift
    private static let opt = CommandModifiers.option
    private static let ctrl = CommandModifiers.control

    /// Table A: shortcuts no row may use.
    public static let blocked: [SystemShortcut] = {
        var rows: [SystemShortcut] = [
            SystemShortcut("q", cmd, "Quit", .hig),
            SystemShortcut("h", cmd, "Hide", .hig),
            SystemShortcut("h", [cmd, opt], "Hide Others", .hig),
            SystemShortcut(",", cmd, "Settings", .hig),
            SystemShortcut(".", cmd, "Cancel", .hig),
            SystemShortcut("w", cmd, "Close", .hig),
            SystemShortcut("w", [cmd, opt], "Close All", .hig),
            SystemShortcut("m", cmd, "Minimize", .hig),
            SystemShortcut("m", [cmd, opt], "Minimize All", .macOS),
            SystemShortcut("`", cmd, "Next window of the app", .macOS),
            SystemShortcut("`", [cmd, shift], "Previous window of the app", .macOS),
            SystemShortcut("tab", cmd, "Next app", .macOS),
            SystemShortcut("tab", [cmd, shift], "Previous app", .macOS),
            SystemShortcut("space", cmd, "Spotlight", .macOS),
            SystemShortcut("space", [cmd, opt], "Finder search window", .macOS),
            SystemShortcut("space", ctrl, "Next input source", .macOS),
            SystemShortcut("space", [ctrl, opt], "Previous input source", .macOS),
            SystemShortcut("escape", [cmd, opt], "Force Quit", .macOS),
            SystemShortcut("q", [ctrl, cmd], "Lock Screen", .macOS),
            SystemShortcut("q", [cmd, shift], "Log Out", .macOS),
            SystemShortcut("q", [cmd, shift, opt], "Log Out now", .macOS),
            SystemShortcut("q", [cmd, opt], "Quit and Keep Windows", .probe),
            SystemShortcut("d", [cmd, opt], "Show or hide the Dock", .macOS),
            SystemShortcut("d", [ctrl, cmd], "Look Up", .macOS),
            SystemShortcut("space", [ctrl, cmd], "Emoji and Symbols", .macOS),
            SystemShortcut("f", [ctrl, cmd], "Full Screen", .hig),
            SystemShortcut("s", [ctrl, cmd], "Show Sidebar", .swiftUI),
            SystemShortcut("t", [cmd, opt], "Show Toolbar", .hig),
            SystemShortcut("z", cmd, "Undo", .hig),
            SystemShortcut("z", [cmd, shift], "Redo", .hig),
            SystemShortcut("x", cmd, "Cut", .hig),
            SystemShortcut("c", cmd, "Copy", .hig),
            SystemShortcut("v", cmd, "Paste", .hig),
            SystemShortcut("v", [cmd, shift, opt], "Paste and Match Style", .hig),
            SystemShortcut("a", cmd, "Select All", .hig),
            SystemShortcut(";", cmd, "Check Spelling", .hig),
            SystemShortcut(":", cmd, "Spelling and Grammar", .hig),
            SystemShortcut("up", ctrl, "Mission Control", .macOS),
            SystemShortcut("down", ctrl, "Application windows", .macOS),
            SystemShortcut("left", ctrl, "Previous Space", .macOS),
            SystemShortcut("right", ctrl, "Next Space", .macOS),
        ]
        for digit in ["3", "4", "5", "6"] {
            rows.append(SystemShortcut(digit, [cmd, shift], "Screenshot", .macOS))
        }
        for number in 1...8 {
            rows.append(SystemShortcut("f\(number)", ctrl, "Keyboard focus", .macOS))
        }
        return rows
    }()

    /// Table B: shortcuts with a standard meaning. A row may use one for the same meaning;
    /// for another meaning it needs an `exception`.
    public static let standard: [SystemShortcut] = [
        SystemShortcut("n", cmd, "New", .hig, meaning: .new),
        SystemShortcut("o", cmd, "Open", .hig, meaning: .open),
        SystemShortcut("d", cmd, "Duplicate", .hig, meaning: .duplicate),
        SystemShortcut("f", cmd, "Find", .hig, meaning: .find),
        SystemShortcut("g", cmd, "Find Next", .hig, meaning: .findNext),
        SystemShortcut("g", [cmd, shift], "Find Previous", .hig, meaning: .findPrevious),
        SystemShortcut("e", cmd, "Use Selection for Find", .hig, meaning: .useSelectionForFind),
        SystemShortcut("j", cmd, "Jump to Selection", .hig, meaning: .jumpToSelection),
        SystemShortcut("[", cmd, "Back", .hig, meaning: .back),
        SystemShortcut("]", cmd, "Forward", .hig, meaning: .forward),
        SystemShortcut("?", cmd, "Help", .hig, meaning: .help),
        SystemShortcut("s", cmd, "Save", .hig, meaning: .save),
        SystemShortcut("s", [cmd, shift], "Save As", .hig, meaning: .saveAs),
        SystemShortcut("p", cmd, "Print", .hig, meaning: .print),
        SystemShortcut("p", [cmd, shift], "Page Setup", .hig, meaning: .pageSetup),
        SystemShortcut("+", cmd, "Zoom In", .hig, meaning: .zoomIn),
        SystemShortcut("-", cmd, "Zoom Out", .hig, meaning: .zoomOut),
        SystemShortcut("0", cmd, "Actual Size", .hig, meaning: .actualSize),
    ]

    /// Table C: the shortcuts of the Format menu Stackly never has. A row may use one only
    /// with an `exception` that gives the reason.
    public static let formatting: [SystemShortcut] = [
        SystemShortcut("b", cmd, "Bold", .hig),
        SystemShortcut("i", cmd, "Italic", .hig),
        SystemShortcut("u", cmd, "Underline", .hig),
        SystemShortcut("t", cmd, "Show Fonts", .hig),
        SystemShortcut("c", [cmd, shift], "Show Colors", .hig),
        SystemShortcut("r", cmd, "Show Ruler", .hig),
        SystemShortcut("c", [cmd, opt], "Copy Style", .hig),
        SystemShortcut("v", [cmd, opt], "Paste Style", .hig),
        SystemShortcut("{", cmd, "Align Left", .hig),
        SystemShortcut("}", cmd, "Align Right", .hig),
        SystemShortcut("|", cmd, "Center", .hig),
    ]

    /// Rule D: whether the shortcut is a key a text field handles itself. A menu key
    /// equivalent is matched before the field sees the key, so such a shortcut would steal
    /// "delete to the start of the line" and the like.
    ///
    /// Arrows and the two delete keys with ⌘ or ⌥ count, so do ⌃ with a letter and no ⌘
    /// (the Emacs bindings of AppKit, such as ⌃A and ⌃K), and so do Return, Escape, Tab,
    /// and Space with or without a modifier.
    ///
    /// - Parameter shortcut: The shortcut to check.
    /// - Returns: `true` when a row may not use it.
    public static func isTextEditingKey(_ shortcut: CommandShortcut) -> Bool {
        let key = shortcut.key
        let modifiers = shortcut.modifiers
        if ["return", "escape", "tab", "space"].contains(key) { return true }
        if ["left", "right", "up", "down", "delete", "forwardDelete"].contains(key) {
            return !modifiers.isDisjoint(with: [.command, .option])
        }
        let isLetter = key.count == 1 && key.first?.isLetter == true
        return isLetter && modifiers.contains(.control) && !modifiers.contains(.command)
    }

    /// The entry of table A with this shortcut.
    ///
    /// - Parameter shortcut: The shortcut.
    /// - Returns: The blocked entry, or `nil`.
    public static func blockedEntry(for shortcut: CommandShortcut) -> SystemShortcut? {
        blocked.first { $0.shortcut == shortcut }
    }

    /// What is wrong with a row's shortcut, as sentences; empty when it is fine.
    ///
    /// Covers tables A to C, rule D, and the ellipsis rule (a title ends in "…" exactly when
    /// the command needs input). Rule E needs no check: ``CommandModifiers`` cannot hold the
    /// 🌐 key.
    ///
    /// - Parameter entry: The row.
    /// - Returns: One sentence per problem.
    static func problems(of entry: CommandEntry) -> [String] {
        var found: [String] = []
        let name = entry.id.rawValue
        if entry.title.hasSuffix("…") != entry.needsInput {
            found.append("\(name): the title ends in “…” only when needsInput is true")
        }
        if let alternate = entry.alternateTitle, alternate.hasSuffix("…") != entry.needsInput {
            found.append("\(name): the alternate title ends in “…” only when needsInput is true")
        }
        guard let shortcut = entry.shortcut else { return found }
        let shown = shortcut.display
        if let hit = blockedEntry(for: shortcut) {
            found.append("\(name): \(shown) is blocked (\(hit.title))")
        }
        if isTextEditingKey(shortcut) {
            found.append("\(name): \(shown) is a text editing key")
        }
        let hasReason = entry.exception?.isEmpty == false
        if let hit = standard.first(where: { $0.shortcut == shortcut }) {
            if entry.standardMeaning != hit.meaning && !hasReason {
                found.append(
                    "\(name): \(shown) means \(hit.title) on macOS; use that meaning or write an exception"
                )
            }
        }
        if let hit = formatting.first(where: { $0.shortcut == shortcut }), !hasReason {
            found.append("\(name): \(shown) is \(hit.title) in the Format menu; write an exception")
        }
        return found
    }
}
