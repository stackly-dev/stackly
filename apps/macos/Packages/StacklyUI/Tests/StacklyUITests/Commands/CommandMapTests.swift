import Testing

@testable import StacklyUI

/// The command map and the system shortcut tables of spec 0069 (AC-1, AC-2).
struct CommandMapTests {

    // MARK: rows

    @Test func everyCommandHasOneRowWithItsOwnId() {
        let entries = CommandMap.entries
        #expect(entries.map(\.id) == CommandID.allCases)
        for id in CommandID.allCases {
            #expect(CommandMap.entry(for: id).id == id)
        }
    }

    @Test func commandRawValuesNeverChange() {
        // The UI tests of spec 0068 read these names; add to the end of the list, never edit it.
        #expect(
            CommandID.allCases.map(\.rawValue) == [
                "reconnect", "newInstance", "find", "showOverview", "showInstances",
                "showEnvironments", "showCatalog", "showRuntimes", "showOperations",
                "showDiagnostics", "back", "toggleInspector", "sortInstancesByName",
                "sortInstancesByProvider", "sortInstancesByVersion", "sortInstancesByEndpoints",
                "sortInstancesByStatus", "sortInstancesAscending", "sortInstancesDescending",
                "openInstance", "startInstance", "stopInstance", "restartInstance",
                "reloadInstance", "copyConnectionString", "duplicateInstance", "removeInstance",
                "stacklyHelp", "welcome", "showMainWindow",
            ])
    }

    @Test func aCommandThatChangesItsTitleHasTheSecondTitle() {
        let inspector = CommandMap.entry(for: .toggleInspector)
        #expect(inspector.title == "Show Inspector")
        #expect(inspector.alternateTitle == "Hide Inspector")
        for entry in CommandMap.entries where entry.id != .toggleInspector {
            #expect(entry.alternateTitle == nil, "\(entry.id)")
        }
    }

    @Test func sectionCommandsAreShellScopeAndPageCommandsNameTheirSections() {
        let sections: [CommandID: SidebarSection] = [
            .showOverview: .overview, .showInstances: .instances,
            .showEnvironments: .environments, .showCatalog: .catalog,
            .showRuntimes: .runtimes, .showOperations: .operations,
            .showDiagnostics: .diagnostics,
        ]
        #expect(sections.count == SidebarSection.allCases.count)
        for id in sections.keys {
            #expect(CommandMap.entry(for: id).scope == .shell, "\(id)")
        }
        #expect(CommandMap.entry(for: .find).scope == .page(Set(SidebarSection.allCases)))
        #expect(CommandMap.entry(for: .back).scope == .page([.instances]))
        #expect(CommandMap.entry(for: .welcome).scope == .app)
        #expect(CommandMap.entry(for: .showMainWindow).kind == .system)
    }

    /// The commands whose menu item exists after the Instances page (spec 0022), with New
    /// Instance… of spec 0020 and the detail page of spec 0021 (Open, Back, and Copy Connection
    /// String), Duplicate… of spec 0024, and Welcome to Stackly of the onboarding (spec 0016).
    /// The rest wait for their owner features.
    private static let builtCommands: Set<CommandID> = [
        .reconnect, .newInstance, .showOverview, .showInstances, .showEnvironments,
        .showCatalog,
        .showRuntimes, .showOperations, .showDiagnostics, .toggleInspector,
        .sortInstancesByName, .sortInstancesByProvider, .sortInstancesByVersion,
        .sortInstancesByEndpoints, .sortInstancesByStatus, .sortInstancesAscending,
        .sortInstancesDescending, .startInstance, .stopInstance, .openInstance, .back,
        .copyConnectionString, .duplicateInstance, .welcome,
    ]

    @Test func aRowIsBuiltExactlyWhenItsMenuItemExists() {
        for entry in CommandMap.entries {
            let expected: CommandStatus = Self.builtCommands.contains(entry.id) ? .built : .planned
            #expect(entry.status == expected, "\(entry.id)")
        }
    }

    // MARK: clashes (AC-2)

    // MARK: titles

    // MARK: shortcut form

    // MARK: layout

    @Test func theMenusShowOnlyTheBuiltRows() {
        func ids(_ menu: CommandMenuID, _ submenu: CommandSubmenuID? = nil) -> [[CommandID]] {
            CommandMap.layout(menu: menu, submenu: submenu).map { $0.map(\.id) }
        }
        #expect(ids(.app) == [[.reconnect]])
        #expect(ids(.file) == [[.newInstance]])
        #expect(ids(.edit).isEmpty)
        #expect(ids(.edit, .find).isEmpty)
        #expect(
            ids(.view) == [
                [
                    .showOverview, .showInstances, .showEnvironments, .showCatalog,
                    .showRuntimes, .showOperations, .showDiagnostics,
                ],
                [.back], [.toggleInspector],
            ])
        #expect(
            ids(.view, .sortInstancesBy) == [
                [
                    .sortInstancesByName, .sortInstancesByProvider, .sortInstancesByVersion,
                    .sortInstancesByEndpoints, .sortInstancesByStatus,
                ],
                [.sortInstancesAscending, .sortInstancesDescending],
            ])
        #expect(
            ids(.instance) == [
                [.openInstance], [.startInstance, .stopInstance], [.copyConnectionString],
                [.duplicateInstance],
            ]
        )
        #expect(ids(.help) == [[.welcome]])
        #expect(ids(.window).isEmpty)
    }

    @Test func layoutOfAFullyBuiltMapGroupsRowsInOrder() {
        let built = CommandMap.entries.map { $0.withStatus(.built) }
        func ids(_ menu: CommandMenuID, _ submenu: CommandSubmenuID? = nil) -> [[CommandID]] {
            CommandMap.layout(of: built, menu: menu, submenu: submenu).map { $0.map(\.id) }
        }
        #expect(ids(.app) == [[.reconnect]])
        #expect(ids(.file) == [[.newInstance]])
        #expect(ids(.edit) == [])
        #expect(ids(.edit, .find) == [[.find]])
        #expect(
            ids(.view) == [
                [
                    .showOverview, .showInstances, .showEnvironments, .showCatalog,
                    .showRuntimes, .showOperations, .showDiagnostics,
                ],
                [.back], [.toggleInspector],
            ])
        #expect(
            ids(.view, .sortInstancesBy) == [
                [
                    .sortInstancesByName, .sortInstancesByProvider, .sortInstancesByVersion,
                    .sortInstancesByEndpoints, .sortInstancesByStatus,
                ],
                [.sortInstancesAscending, .sortInstancesDescending],
            ])
        #expect(
            ids(.instance) == [
                [.openInstance],
                [.startInstance, .stopInstance, .restartInstance, .reloadInstance],
                [.copyConnectionString], [.duplicateInstance, .removeInstance],
            ])
        #expect(ids(.help) == [[.stacklyHelp], [.welcome]])
        #expect(ids(.window) == [])
    }

    @Test func layoutLeavesOutGroupsWithoutABuiltRow() {
        var rows = CommandMap.entries.map { $0.withStatus(.built) }
        rows = rows.map { $0.id == .back ? $0.withStatus(.planned) : $0 }
        let groups = CommandMap.layout(of: rows, menu: .view, submenu: nil)
        #expect(groups.map { $0.map(\.id) }.contains([.back]) == false)
        #expect(groups.count == 2)
        #expect(groups.allSatisfy { $0.isEmpty == false })
    }
}

extension CommandEntry {
    /// The same row with another status.
    fileprivate func withStatus(_ status: CommandStatus) -> CommandEntry {
        CommandEntry(
            id, menu: menu, submenu: submenu, group: group, kind: kind, title: title,
            alternateTitle: alternateTitle, needsInput: needsInput,
            scope: scope, requirements: requirements, owner: owner, status: status)
    }
}
