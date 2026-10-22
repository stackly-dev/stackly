import Testing

@testable import StacklyUI

/// The command map and the system shortcut tables of spec 0069 (AC-1, AC-2).
struct CommandMapTests {
    /// A row with the given shortcut, for the rule checks.
    private func row(
        _ shortcut: CommandShortcut?, title: String = "Probe", needsInput: Bool = false,
        meaning: StandardMeaning? = nil, exception: String? = nil
    ) -> CommandEntry {
        CommandEntry(
            .reconnect, menu: .app, group: 1, title: title, shortcut: shortcut,
            needsInput: needsInput, scope: .app, owner: 69, standardMeaning: meaning,
            exception: exception)
    }

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

    @Test func rowsCarryTheShortcutsOfTheSpecTable() {
        let expected: [CommandID: String] = [
            .newInstance: "⌘N", .find: "⌘F", .showOverview: "⌘1", .showInstances: "⌘2",
            .showEnvironments: "⌘3", .showCatalog: "⌘4", .showRuntimes: "⌘5",
            .showOperations: "⌘6", .showDiagnostics: "⌘7", .back: "⌘[",
            .toggleInspector: "⌥⌘I", .openInstance: "⌘O", .startInstance: "⌘R",
            .stopInstance: "⇧⌘R", .restartInstance: "⌥⌘R", .copyConnectionString: "⇧⌘C",
            .duplicateInstance: "⌘D", .stacklyHelp: "⌘?", .showMainWindow: "⌘0",
        ]
        for entry in CommandMap.entries {
            #expect(entry.shortcut?.display == expected[entry.id], "\(entry.id)")
        }
    }

    @Test func aCommandThatChangesItsTitleHasTheSecondTitle() {
        let inspector = CommandMap.entry(for: .toggleInspector)
        #expect(inspector.title == "Show Inspector")
        #expect(inspector.alternateTitle == "Hide Inspector")
        for entry in CommandMap.entries where entry.id != .toggleInspector {
            #expect(entry.alternateTitle == nil, "\(entry.id)")
        }
    }

    @Test func noDestructiveCommandHasAShortcut() {
        #expect(CommandMap.entry(for: .removeInstance).shortcut == nil)
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
    /// String), and Welcome to Stackly of the onboarding (spec 0016). The rest wait for their
    /// owner features.
    private static let builtCommands: Set<CommandID> = [
        .reconnect, .newInstance, .find, .showOverview, .showInstances, .showEnvironments,
        .showCatalog,
        .showRuntimes, .showOperations, .showDiagnostics, .showMainWindow, .toggleInspector,
        .sortInstancesByName, .sortInstancesByProvider, .sortInstancesByVersion,
        .sortInstancesByEndpoints, .sortInstancesByStatus, .sortInstancesAscending,
        .sortInstancesDescending, .startInstance, .stopInstance, .openInstance, .back,
        .copyConnectionString, .welcome,
    ]

    @Test func aRowIsBuiltExactlyWhenItsMenuItemExists() {
        for entry in CommandMap.entries {
            let expected: CommandStatus = Self.builtCommands.contains(entry.id) ? .built : .planned
            #expect(entry.status == expected, "\(entry.id)")
        }
    }

    // MARK: clashes (AC-2)

    @Test func noTwoRowsShareAShortcut() {
        var seen: [CommandShortcut: CommandID] = [:]
        for entry in CommandMap.entries {
            guard let shortcut = entry.shortcut else { continue }
            if let other = seen[shortcut] {
                Issue.record("\(entry.id) and \(other) both use \(shortcut.display)")
            }
            seen[shortcut] = entry.id
        }
    }

    @Test func noRowBreaksAShortcutRule() {
        for entry in CommandMap.entries {
            #expect(
                SystemShortcuts.problems(of: entry).isEmpty,
                "\(SystemShortcuts.problems(of: entry))")
        }
    }

    @Test(arguments: SystemShortcuts.blocked)
    func aRowOnABlockedShortcutIsRefused(blocked: SystemShortcut) {
        let problems = SystemShortcuts.problems(
            of: row(blocked.shortcut, meaning: .new, exception: "no good reason"))
        #expect(problems.contains { $0.contains("is blocked") }, "\(blocked.title)")
    }

    @Test func aShortcutWithAStandardMeaningNeedsTheSameMeaningOrAReason() {
        let commandN = CommandShortcut("n", .command)
        #expect(SystemShortcuts.problems(of: row(commandN, meaning: .new)).isEmpty)
        #expect(SystemShortcuts.problems(of: row(commandN)).count == 1)
        #expect(SystemShortcuts.problems(of: row(commandN, meaning: .open)).count == 1)
        #expect(
            SystemShortcuts.problems(of: row(commandN, exception: "Stackly has no documents"))
                .isEmpty)
        #expect(SystemShortcuts.problems(of: row(commandN, exception: "")).count == 1)
    }

    @Test func aFormatShortcutNeedsAReason() {
        let showColors = CommandShortcut("c", [.command, .shift])
        #expect(SystemShortcuts.problems(of: row(showColors)).count == 1)
        #expect(SystemShortcuts.problems(of: row(showColors, exception: "no Format menu")).isEmpty)
        let bold = CommandShortcut("b", .command)
        #expect(SystemShortcuts.problems(of: row(bold)).count == 1)
    }

    @Test func textEditingKeysAreRefused() {
        let refused: [CommandShortcut] = [
            CommandShortcut("delete", .command), CommandShortcut("delete", .option),
            CommandShortcut("forwardDelete", .option), CommandShortcut("left", .option),
            CommandShortcut("right", [.command, .shift]), CommandShortcut("a", .control),
            CommandShortcut("k", .control), CommandShortcut("e", [.control, .shift]),
            CommandShortcut("return", []), CommandShortcut("return", .command),
            CommandShortcut("escape", []), CommandShortcut("tab", .control),
            CommandShortcut("space", .option),
        ]
        for shortcut in refused {
            #expect(SystemShortcuts.isTextEditingKey(shortcut), "\(shortcut.display)")
            #expect(
                SystemShortcuts.problems(of: row(shortcut, exception: "reason"))
                    .contains { $0.contains("text editing key") })
        }
    }

    @Test func shortcutsThatNeedNoTextFieldKeyAreNotTextEditingKeys() {
        let fine: [CommandShortcut] = [
            CommandShortcut("r", [.command, .option]), CommandShortcut("1", .command),
            CommandShortcut("g", [.control, .command]), CommandShortcut("[", .command),
            CommandShortcut("up", .shift),
        ]
        for shortcut in fine {
            #expect(SystemShortcuts.isTextEditingKey(shortcut) == false, "\(shortcut.display)")
        }
    }

    @Test func theFunctionModifierCannotBeWritten() {
        // Rule E: CommandModifiers has the four letters of the Apple order and no 🌐.
        #expect(CommandModifiers.all.rawValue == 0b1111)
    }

    @Test func theSystemTablesHaveNoClashWithinOrBetweenThem() {
        let tables = [
            ("A", SystemShortcuts.blocked), ("B", SystemShortcuts.standard),
            ("C", SystemShortcuts.formatting),
        ]
        var seen: [CommandShortcut: String] = [:]
        for (name, table) in tables {
            for item in table {
                if let other = seen[item.shortcut] {
                    Issue.record(
                        "\(item.title) in table \(name) repeats a shortcut of table \(other)")
                }
                seen[item.shortcut] = name
            }
        }
    }

    @Test func everyStandardMeaningRowIsInTableB() {
        for entry in CommandMap.entries {
            guard let meaning = entry.standardMeaning else { continue }
            let match = SystemShortcuts.standard.first { $0.shortcut == entry.shortcut }
            #expect(match?.meaning == meaning, "\(entry.id)")
        }
    }

    // MARK: titles

    @Test func titleEndsInAnEllipsisExactlyWhenTheCommandNeedsInput() {
        for entry in CommandMap.entries {
            #expect(entry.title.hasSuffix("…") == entry.needsInput, "\(entry.id)")
        }
        #expect(SystemShortcuts.problems(of: row(nil, title: "Open…")).count == 1)
        #expect(SystemShortcuts.problems(of: row(nil, title: "Open", needsInput: true)).count == 1)
        #expect(SystemShortcuts.problems(of: row(nil, title: "Open…", needsInput: true)).isEmpty)
    }

    // MARK: shortcut form

    @Test func shortcutsShowModifiersInAppleOrder() {
        #expect(CommandShortcut("r", [.command, .shift]).display == "⇧⌘R")
        #expect(CommandShortcut("i", [.command, .option]).display == "⌥⌘I")
        #expect(CommandShortcut("g", [.command, .control]).display == "⌃⌘G")
        #expect(CommandShortcut("q", [.command, .shift, .option, .control]).display == "⌃⌥⇧⌘Q")
        #expect(CommandShortcut("tab", .command).display == "⌘⇥")
    }

    @Test func shiftedPunctuationIsStoredAsItsBaseKeyAndShownShifted() {
        let help = CommandShortcut("?", .command)
        #expect(help.key == "/")
        #expect(help.modifiers == [.command, .shift])
        #expect(help.display == "⌘?")
        #expect(help == CommandShortcut("/", [.command, .shift]))
        #expect(CommandShortcut("{", .command).display == "⌘{")
        #expect(CommandShortcut("+", .command).display == "⌘+")
        #expect(CommandShortcut("1", [.command, .shift]).display == "⇧⌘1")
    }

    @Test func lettersAreStoredLowercaseSoCapitalsCompareEqual() {
        #expect(CommandShortcut("R", .command) == CommandShortcut("r", .command))
    }

    @Test func helpTextShowsTheShortcutInParentheses() {
        #expect(CommandMap.helpText(for: .toggleInspector) == "Show Inspector (⌥⌘I)")
        #expect(
            CommandMap.helpText(for: .toggleInspector, title: "Hide Inspector")
                == "Hide Inspector (⌥⌘I)")
        #expect(CommandMap.helpText(for: .reconnect) == "Reconnect to stacklyd")
        #expect(CommandMap.helpText(for: .stacklyHelp) == "Stackly Help (⌘?)")
    }

    // MARK: layout

    @Test func theMenusShowOnlyTheBuiltRows() {
        func ids(_ menu: CommandMenuID, _ submenu: CommandSubmenuID? = nil) -> [[CommandID]] {
            CommandMap.layout(menu: menu, submenu: submenu).map { $0.map(\.id) }
        }
        #expect(ids(.app) == [[.reconnect]])
        #expect(ids(.file) == [[.newInstance]])
        #expect(ids(.edit).isEmpty)
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
                [.openInstance], [.startInstance, .stopInstance], [.copyConnectionString],
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
            alternateTitle: alternateTitle, shortcut: shortcut, needsInput: needsInput,
            scope: scope, requirements: requirements, owner: owner, status: status,
            standardMeaning: standardMeaning, exception: exception)
    }
}
