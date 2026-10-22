import Foundation

/// The identity of a command in the menu bar of spec 0069.
///
/// The cases are declared in menu order, and the raw values never change after they are
/// used: the UI tests of spec 0068 read them.
nonisolated public enum CommandID: String, CaseIterable, Hashable, Sendable {
    /// Stackly > Reconnect to stacklyd.
    case reconnect
    /// File > New Instance….
    case newInstance
    /// Edit > Find > Find….
    case find
    /// View > Overview.
    case showOverview
    /// View > Instances.
    case showInstances
    /// View > Environments.
    case showEnvironments
    /// View > Catalog.
    case showCatalog
    /// View > Runtimes.
    case showRuntimes
    /// View > Operations.
    case showOperations
    /// View > Diagnostics.
    case showDiagnostics
    /// View > Back.
    case back
    /// View > Show Inspector, or Hide Inspector.
    case toggleInspector
    /// View > Sort Instances By > Name.
    case sortInstancesByName
    /// View > Sort Instances By > Provider.
    case sortInstancesByProvider
    /// View > Sort Instances By > Version.
    case sortInstancesByVersion
    /// View > Sort Instances By > Endpoints.
    case sortInstancesByEndpoints
    /// View > Sort Instances By > Status.
    case sortInstancesByStatus
    /// View > Sort Instances By > Ascending.
    case sortInstancesAscending
    /// View > Sort Instances By > Descending.
    case sortInstancesDescending
    /// Instance > Open.
    case openInstance
    /// Instance > Start.
    case startInstance
    /// Instance > Stop.
    case stopInstance
    /// Instance > Restart.
    case restartInstance
    /// Instance > Reload.
    case reloadInstance
    /// Instance > Copy Connection String.
    case copyConnectionString
    /// Instance > Duplicate….
    case duplicateInstance
    /// Instance > Remove….
    case removeInstance
    /// Help > Stackly Help.
    case stacklyHelp
    /// Help > Welcome to Stackly.
    case welcome
    /// Window > Stackly, the item the main `Window` scene adds itself.
    case showMainWindow
}

/// A menu of the menu bar, in the order SwiftUI places them.
nonisolated public enum CommandMenuID: String, CaseIterable, Sendable {
    /// The application menu, titled Stackly.
    case app
    /// File.
    case file
    /// Edit.
    case edit
    /// View.
    case view
    /// Instance, between View and Window.
    case instance
    /// Window.
    case window
    /// Help.
    case help
}

/// A submenu a command can sit in.
nonisolated public enum CommandSubmenuID: String, CaseIterable, Sendable {
    /// Edit > Find.
    case find
    /// View > Sort Instances By.
    case sortInstancesBy
}

/// How a command is drawn in its menu.
nonisolated public enum CommandKind: String, Hashable, Sendable {
    /// A `Button`.
    case button
    /// A menu `Toggle` with a checkmark.
    case toggle
    /// An item SwiftUI or macOS creates itself; the map only keeps its shortcut.
    case system
}

/// The modifier keys of a shortcut. macOS has no modifier for the 🌐 key here, so a
/// shortcut that needs it cannot be written (rule E of spec 0069).
nonisolated public struct CommandModifiers: OptionSet, Hashable, Sendable {
    /// The bits of the set.
    public let rawValue: Int

    /// Creates a set from its bits.
    public init(rawValue: Int) {
        self.rawValue = rawValue
    }

    /// ⌘.
    public static let command = CommandModifiers(rawValue: 1 << 0)
    /// ⇧.
    public static let shift = CommandModifiers(rawValue: 1 << 1)
    /// ⌥.
    public static let option = CommandModifiers(rawValue: 1 << 2)
    /// ⌃.
    public static let control = CommandModifiers(rawValue: 1 << 3)

    /// Every modifier that can be written.
    public static let all: CommandModifiers = [.command, .shift, .option, .control]
}

/// A keyboard shortcut in the normal form the clash tests compare.
///
/// The key is one lowercase character (`"r"`, `"1"`, `"["`) or the name of a special key
/// (`"tab"`, `"space"`, `"escape"`, `"return"`, `"delete"`, `"forwardDelete"`, `"up"`,
/// `"down"`, `"left"`, `"right"`, `"f1"` to `"f12"`). Punctuation that needs ⇧ is stored as
/// its base key with ⇧, so ⌘? is `"/"` with ⇧⌘.
nonisolated public struct CommandShortcut: Hashable, Sendable {
    /// The base key.
    public let key: String
    /// The modifiers, with ⇧ written out also for shifted punctuation.
    public let modifiers: CommandModifiers

    /// Pairs of a base punctuation key and the character ⇧ turns it into.
    private static let shiftedPunctuation: [(base: String, shifted: String)] = [
        ("/", "?"), ("[", "{"), ("]", "}"), ("\\", "|"), (";", ":"), (",", "<"), (".", ">"),
        ("'", "\""), ("-", "_"), ("=", "+"), ("`", "~"),
    ]

    /// The character ⇧ turns a base punctuation key into, or `nil` for any other key.
    static func shiftedCharacter(ofBaseKey key: String) -> Character? {
        shiftedPunctuation.first { $0.base == key }.flatMap { $0.shifted.first }
    }

    /// The glyph or label a special key shows in a menu.
    private static let specialKeyDisplay: [String: String] = [
        "tab": "⇥", "space": "Space", "escape": "⎋", "return": "↩", "delete": "⌫",
        "forwardDelete": "⌦", "up": "↑", "down": "↓", "left": "←", "right": "→",
    ]

    /// Creates a shortcut and brings it to its normal form: letters lowercase, shifted
    /// punctuation as its base key with ⇧.
    ///
    /// - Parameters:
    ///   - key: The key, or the shifted character such as `"?"`.
    ///   - modifiers: The modifiers.
    public init(_ key: String, _ modifiers: CommandModifiers) {
        if let pair = Self.shiftedPunctuation.first(where: { $0.shifted == key }) {
            self.key = pair.base
            self.modifiers = modifiers.union(.shift)
        } else {
            self.key = key.count == 1 ? key.lowercased() : key
            self.modifiers = modifiers
        }
    }

    /// The shortcut as a menu shows it: modifiers in the order ⌃⌥⇧⌘, then the key in
    /// capitals. Shifted punctuation shows as its shifted character without ⇧.
    public var display: String {
        var shown = modifiers
        var keyText = Self.specialKeyDisplay[key] ?? key.uppercased()
        if modifiers.contains(.shift),
            let pair = Self.shiftedPunctuation.first(where: { $0.base == key })
        {
            shown.remove(.shift)
            keyText = pair.shifted
        }
        var text = ""
        if shown.contains(.control) { text += "⌃" }
        if shown.contains(.option) { text += "⌥" }
        if shown.contains(.shift) { text += "⇧" }
        if shown.contains(.command) { text += "⌘" }
        return text + keyText
    }
}

/// Where a command is active.
nonisolated public enum CommandScope: Hashable, Sendable {
    /// Always, also with the main window closed and while onboarding shows.
    case app
    /// While the main window is key and the shell shows.
    case shell
    /// While the main window is key and one of these sections shows.
    case page(Set<SidebarSection>)
}

/// What a command needs before it can run. Descriptive: the published action decides.
nonisolated public enum CommandRequirement: String, Hashable, Sendable {
    /// The action changes data, so the mutation gate applies.
    case daemon
    /// An instance is selected, or its detail is open.
    case selection
    /// An instance detail is open.
    case detail
    /// The page has a search field.
    case search
    /// The daemon is not connected.
    case noConnection
}

/// Whether the menu builds a command yet.
nonisolated public enum CommandStatus: Hashable, Sendable {
    /// A `CommandItem` renders it.
    case built
    /// Reserved for a feature that has not built it; the menu does not show it.
    case planned
}

/// The standard meaning a shortcut has on macOS, so a row may reuse it for the same job.
nonisolated public enum StandardMeaning: String, Hashable, Sendable {
    /// New.
    case new
    /// Open.
    case open
    /// Duplicate.
    case duplicate
    /// Find.
    case find
    /// Find Next.
    case findNext
    /// Find Previous.
    case findPrevious
    /// Use Selection for Find.
    case useSelectionForFind
    /// Jump to Selection.
    case jumpToSelection
    /// Back.
    case back
    /// Forward.
    case forward
    /// Help.
    case help
    /// Save.
    case save
    /// Save As.
    case saveAs
    /// Print.
    case print
    /// Page Setup.
    case pageSetup
    /// Zoom In.
    case zoomIn
    /// Zoom Out.
    case zoomOut
    /// Actual Size.
    case actualSize
}

/// One row of the command map: everything the menu and the clash tests know about a
/// command.
nonisolated public struct CommandEntry: Hashable, Identifiable, Sendable {
    /// The command.
    public let id: CommandID
    /// The menu the command sits in.
    public let menu: CommandMenuID
    /// The submenu inside that menu, or `nil` for the menu itself.
    public let submenu: CommandSubmenuID?
    /// The group inside the menu or submenu; groups are separated by a divider. Zero for a
    /// `system` command.
    public let group: Int
    /// How the command is drawn.
    public let kind: CommandKind
    /// The menu title. It ends in "…" exactly when ``needsInput`` is true.
    public let title: String
    /// The second title of a command whose title changes, such as Hide Inspector.
    public let alternateTitle: String?
    /// The shortcut, or `nil` when the command has none.
    public let shortcut: CommandShortcut?
    /// Whether the command asks for input before it acts.
    public let needsInput: Bool
    /// Where the command is active.
    public let scope: CommandScope
    /// What the command needs.
    public let requirements: Set<CommandRequirement>
    /// The number of the feature that owns the command.
    public let owner: Int
    /// Whether the menu builds the command yet.
    public let status: CommandStatus
    /// The standard meaning of the shortcut, when the command uses one.
    public let standardMeaning: StandardMeaning?
    /// Why the command may use a shortcut that means something else on macOS.
    public let exception: String?

    /// Creates a row.
    init(
        _ id: CommandID,
        menu: CommandMenuID,
        submenu: CommandSubmenuID? = nil,
        group: Int,
        kind: CommandKind = .button,
        title: String,
        alternateTitle: String? = nil,
        shortcut: CommandShortcut? = nil,
        needsInput: Bool = false,
        scope: CommandScope,
        requirements: Set<CommandRequirement> = [],
        owner: Int,
        status: CommandStatus = .planned,
        standardMeaning: StandardMeaning? = nil,
        exception: String? = nil
    ) {
        self.id = id
        self.menu = menu
        self.submenu = submenu
        self.group = group
        self.kind = kind
        self.title = title
        self.alternateTitle = alternateTitle
        self.shortcut = shortcut
        self.needsInput = needsInput
        self.scope = scope
        self.requirements = requirements
        self.owner = owner
        self.status = status
        self.standardMeaning = standardMeaning
        self.exception = exception
    }
}

/// The single source of menu titles and shortcuts of Stackly (spec 0069).
///
/// A shortcut is written here and nowhere else; the unit tests refuse a clash with another
/// row or with a macOS shortcut. The status of every row is `planned` until the build step
/// that adds its `CommandItem` turns it into `built`.
nonisolated public enum CommandMap {
    /// The row of a command.
    ///
    /// - Parameter id: The command.
    /// - Returns: Its row. The `switch` covers every case, so adding a ``CommandID`` fails
    ///   to compile until it has a row.
    public static func entry(for id: CommandID) -> CommandEntry {
        switch id {
        case .reconnect:
            CommandEntry(
                id, menu: .app, group: 1, title: "Reconnect to stacklyd",
                scope: .shell, requirements: [.noConnection], owner: 69, status: .built)
        case .newInstance:
            CommandEntry(
                id, menu: .file, group: 1, title: "New Instance…",
                shortcut: CommandShortcut("n", .command), needsInput: true,
                scope: .shell, requirements: [.daemon], owner: 20, standardMeaning: .new)
        case .find:
            CommandEntry(
                id, menu: .edit, submenu: .find, group: 1, title: "Find…",
                shortcut: CommandShortcut("f", .command), needsInput: true,
                scope: .page(Set(SidebarSection.allCases)), requirements: [.search],
                owner: 69, status: .built, standardMeaning: .find)
        case .showOverview:
            showSection(id, title: "Overview", key: "1")
        case .showInstances:
            showSection(id, title: "Instances", key: "2")
        case .showEnvironments:
            showSection(id, title: "Environments", key: "3")
        case .showCatalog:
            showSection(id, title: "Catalog", key: "4")
        case .showRuntimes:
            showSection(id, title: "Runtimes", key: "5")
        case .showOperations:
            showSection(id, title: "Operations", key: "6")
        case .showDiagnostics:
            showSection(id, title: "Diagnostics", key: "7")
        case .back:
            CommandEntry(
                id, menu: .view, group: 2, title: "Back",
                shortcut: CommandShortcut("[", .command), scope: instancesPage,
                requirements: [.detail], owner: 21, status: .built, standardMeaning: .back)
        case .toggleInspector:
            CommandEntry(
                id, menu: .view, group: 3, title: "Show Inspector",
                alternateTitle: "Hide Inspector",
                shortcut: CommandShortcut("i", [.command, .option]), scope: instancesPage,
                owner: 22, status: .built)
        case .sortInstancesByName:
            sortKey(id, title: "Name")
        case .sortInstancesByProvider:
            sortKey(id, title: "Provider")
        case .sortInstancesByVersion:
            sortKey(id, title: "Version")
        case .sortInstancesByEndpoints:
            sortKey(id, title: "Endpoints")
        case .sortInstancesByStatus:
            sortKey(id, title: "Status")
        case .sortInstancesAscending:
            sortDirection(id, title: "Ascending")
        case .sortInstancesDescending:
            sortDirection(id, title: "Descending")
        case .openInstance:
            CommandEntry(
                id, menu: .instance, group: 1, title: "Open",
                shortcut: CommandShortcut("o", .command), scope: instancesPage,
                requirements: [.selection], owner: 21, status: .built, standardMeaning: .open)
        case .startInstance:
            CommandEntry(
                id, menu: .instance, group: 2, title: "Start",
                shortcut: CommandShortcut("r", .command), scope: instancesPage,
                requirements: [.selection, .daemon], owner: 22, status: .built,
                exception:
                    "⌘R is Show Ruler in the Format menu; Stackly has no Format menu, ruler, or styled text"
            )
        case .stopInstance:
            CommandEntry(
                id, menu: .instance, group: 2, title: "Stop",
                shortcut: CommandShortcut("r", [.command, .shift]), scope: instancesPage,
                requirements: [.selection, .daemon], owner: 22, status: .built)
        case .restartInstance:
            CommandEntry(
                id, menu: .instance, group: 2, title: "Restart",
                shortcut: CommandShortcut("r", [.command, .option]), scope: instancesPage,
                requirements: [.selection, .daemon], owner: 26)
        case .reloadInstance:
            CommandEntry(
                id, menu: .instance, group: 2, title: "Reload", scope: instancesPage,
                requirements: [.selection, .daemon], owner: 26)
        case .copyConnectionString:
            CommandEntry(
                id, menu: .instance, group: 3, title: "Copy Connection String",
                shortcut: CommandShortcut("c", [.command, .shift]), scope: instancesPage,
                requirements: [.selection], owner: 21, status: .built,
                exception:
                    "⇧⌘C is Show Colors in the Format menu; Stackly has no Format menu or color panel"
            )
        case .duplicateInstance:
            CommandEntry(
                id, menu: .instance, group: 4, title: "Duplicate…",
                shortcut: CommandShortcut("d", .command), needsInput: true,
                scope: instancesPage, requirements: [.selection, .daemon], owner: 24,
                standardMeaning: .duplicate)
        case .removeInstance:
            CommandEntry(
                id, menu: .instance, group: 4, title: "Remove…", needsInput: true,
                scope: instancesPage, requirements: [.selection, .daemon], owner: 54)
        case .stacklyHelp:
            CommandEntry(
                id, menu: .help, group: 1, title: "Stackly Help",
                shortcut: CommandShortcut("?", .command), scope: .app, owner: 66,
                standardMeaning: .help)
        case .welcome:
            CommandEntry(
                id, menu: .help, group: 2, title: "Welcome to Stackly", scope: .app, owner: 16)
        case .showMainWindow:
            CommandEntry(
                id, menu: .window, group: 0, kind: .system, title: "Stackly",
                shortcut: CommandShortcut("0", .command), scope: .app, owner: 69,
                status: .built,
                exception:
                    "⌘0 is Actual Size; Stackly has no zoom, and Mail uses ⌘0 for its main window")
        }
    }

    /// Every row, in declaration order.
    public static var entries: [CommandEntry] {
        CommandID.allCases.map(entry(for:))
    }

    /// The built rows of a menu or submenu, as groups in group order. Inside a group the
    /// rows keep declaration order. Groups with no built row are left out, and so are
    /// `system` rows, which the menu builder does not render.
    ///
    /// - Parameters:
    ///   - menu: The menu.
    ///   - submenu: The submenu, or `nil` for the menu itself.
    /// - Returns: The groups; the builder puts a divider between two of them.
    public static func layout(menu: CommandMenuID, submenu: CommandSubmenuID?) -> [[CommandEntry]] {
        layout(of: entries, menu: menu, submenu: submenu)
    }

    /// The same as ``layout(menu:submenu:)`` for a given list of rows, so tests can lay out
    /// a map in which every row is built.
    static func layout(
        of rows: [CommandEntry], menu: CommandMenuID, submenu: CommandSubmenuID?
    ) -> [[CommandEntry]] {
        let wanted = rows.filter {
            $0.menu == menu && $0.submenu == submenu && $0.status == .built && $0.kind != .system
        }
        return Dictionary(grouping: wanted, by: \.group)
            .sorted { $0.key < $1.key }
            .map(\.value)
    }

    /// The text of a button's tooltip: the title and, in parentheses, the shortcut.
    ///
    /// - Parameters:
    ///   - id: The command.
    ///   - title: The title to show instead of the row's, for a command whose title changes.
    /// - Returns: "Hide Inspector (⌥⌘I)", or the title alone when the command has no shortcut.
    public static func helpText(for id: CommandID, title: String? = nil) -> String {
        let row = entry(for: id)
        let shown = title ?? row.title
        guard let shortcut = row.shortcut else { return shown }
        return "\(shown) (\(shortcut.display))"
    }

    /// The Instances page only.
    private static let instancesPage: CommandScope = .page([.instances])

    /// A View > section row.
    private static func showSection(_ id: CommandID, title: String, key: String) -> CommandEntry {
        CommandEntry(
            id, menu: .view, group: 1, title: title,
            shortcut: CommandShortcut(key, .command), scope: .shell, owner: 69,
            status: .built)
    }

    /// A View > Sort Instances By > column row.
    private static func sortKey(_ id: CommandID, title: String) -> CommandEntry {
        CommandEntry(
            id, menu: .view, submenu: .sortInstancesBy, group: 1, kind: .toggle, title: title,
            scope: instancesPage, owner: 22, status: .built)
    }

    /// A View > Sort Instances By > direction row.
    private static func sortDirection(_ id: CommandID, title: String) -> CommandEntry {
        CommandEntry(
            id, menu: .view, submenu: .sortInstancesBy, group: 2, kind: .toggle, title: title,
            scope: instancesPage, owner: 22, status: .built)
    }
}
