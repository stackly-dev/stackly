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
        // Before Services, not after the Settings group: AX shows the Settings… item the
        // scene adds comes after a group placed after `.appSettings` (macOS 27.0.1).
        CommandGroup(before: .systemServices) {
            CommandItem(.reconnect, context: context)
            Divider()
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
            GroupDivider(.view, before: 2)
            CommandItem(.back, context: context)
            GroupDivider(.view, before: 3)
            CommandItem(.toggleInspector, context: context)
            Menu("Sort Instances By") {
                CommandItem(.sortInstancesByName, context: context)
                CommandItem(.sortInstancesByProvider, context: context)
                CommandItem(.sortInstancesByVersion, context: context)
                CommandItem(.sortInstancesByEndpoints, context: context)
                CommandItem(.sortInstancesByStatus, context: context)
                Divider()
                CommandItem(.sortInstancesAscending, context: context)
                CommandItem(.sortInstancesDescending, context: context)
            }
        }
        CommandMenu("Instance") {
            CommandItem(.openInstance, context: context)
            GroupDivider(.instance, before: 2)
            CommandItem(.startInstance, context: context)
            CommandItem(.stopInstance, context: context)
            GroupDivider(.instance, before: 3)
            CommandItem(.copyConnectionString, context: context)
        }
        // Replaced so the Stackly Help item that opens nothing goes away; the search field
        // stays. Welcome to Stackly joins with spec 0016.
        CommandGroup(replacing: .help) {}
    }
}
