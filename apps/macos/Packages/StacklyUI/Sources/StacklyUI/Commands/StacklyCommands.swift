import SwiftUI

/// The menu bar of Stackly (spec 0069): the items the app adds to the menus SwiftUI and
/// macOS build, in the places of the *Struktur menu* table.
///
/// Every item comes from ``CommandMap`` through ``CommandItem``, so a title or a shortcut is
/// written in one place. The group SwiftUI builds for New Window, text editing, and Help are
/// replaced, so the menus have no item Stackly cannot run. Add `.commands { … }` once, on the
/// main window scene.
public struct StacklyCommands: Commands {
    @FocusedValue(\.shellCommands) private var shell
    @FocusedValue(\.pageCommands) private var page

    private let appActions: [CommandID: @MainActor () -> Void]

    /// Creates the commands.
    ///
    /// - Parameter runtime: The shell runtime the items that are always on act on.
    public init(runtime: ShellRuntime) {
        appActions = Self.appActions(runtime: runtime)
    }

    /// The actions of the `app` scope items of kind `button`. They are always on, also with
    /// the main window closed.
    ///
    /// None is built yet: Welcome to Stackly joins with the onboarding of spec 0016, and
    /// Stackly Help with feature 66.
    static func appActions(runtime: ShellRuntime) -> [CommandID: @MainActor () -> Void] {
        [:]
    }

    private var context: CommandContext {
        CommandContext(shell: shell, page: page, appActions: appActions)
    }

    /// The commands.
    public var body: some Commands {
        CommandGroup(after: .appSettings) {
            Divider()
            CommandItem(.reconnect, context: context)
        }
        // Replaced so New Window goes away; File > New Instance… joins with spec 0020.
        CommandGroup(replacing: .newItem) {}
        CommandGroup(replacing: .textEditing) {
            Menu("Find") {
                CommandItem(.find, context: context)
            }
        }
        SidebarCommands()
        CommandGroup(after: .sidebar) {
            Divider()
            CommandItem(.showOverview, context: context)
            CommandItem(.showInstances, context: context)
            CommandItem(.showEnvironments, context: context)
            CommandItem(.showCatalog, context: context)
            CommandItem(.showRuntimes, context: context)
            CommandItem(.showOperations, context: context)
            CommandItem(.showDiagnostics, context: context)
        }
        // Replaced so the Stackly Help item that opens nothing goes away; the search field
        // stays. Welcome to Stackly joins with spec 0016.
        CommandGroup(replacing: .help) {}
    }
}
