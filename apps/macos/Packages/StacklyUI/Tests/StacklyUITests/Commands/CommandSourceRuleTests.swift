import Foundation
import Testing

@testable import StacklyUI

/// The source rules of spec 0069 (AC-1): a shortcut and a menu live only in the command
/// folder and the app's scene file, and every built command has exactly one menu item.
///
/// This suite reuses the `Rule` type and the source scan of `SourceRuleTests`, so the scan
/// is text based and skips line comments. Like that suite it is `nonisolated` and
/// serialized, so the scans stay off the main actor.
@Suite(.serialized) nonisolated struct CommandSourceRuleTests {
    nonisolated static let commandsFolder = SourceRuleTests.stacklyUI + "Commands/"
    nonisolated static let appScene = "apps/macos/Stackly/StacklyApp.swift"

    nonisolated static let rules = [
        SourceRuleTests.Rule(
            name: "keyboard shortcut outside the command map (spec 0069 AC-1)",
            patterns: [
                #"\.keyboardShortcut\((?!\s*(\.(defaultAction|cancelAction)\b|CommandMap\b))"#
            ],
            allowed: [commandsFolder, appScene],
            forbiddenExamples: [
                #".keyboardShortcut("r")"#,
                ".keyboardShortcut(KeyEquivalent(\"n\"), modifiers: .command)",
                ".keyboardShortcut(.return)",
            ],
            allowedExamples: [
                ".keyboardShortcut(.defaultAction)",
                ".keyboardShortcut(.cancelAction)",
                ".keyboardShortcut(CommandMap.entry(for: .showMainWindow).keyboardShortcut)",
            ]
        ),
        SourceRuleTests.Rule(
            name: "menu or commands scene modifier outside the command folder (spec 0069 AC-1)",
            patterns: [#"\bCommandGroup\b"#, #"\bCommandMenu\b"#, #"\.commands\b"#],
            allowed: [commandsFolder, appScene],
            forbiddenExamples: [
                "CommandGroup(replacing: .newItem) {}",
                "CommandMenu(\"Instance\") {}",
                ".commands { StacklyCommands(runtime: runtime) }",
            ],
            allowedExamples: [
                "struct StacklyCommands: Commands {",
                "let commandsFolder = 1",
            ]
        ),
    ]

    @Test(arguments: rules)
    func sourcesFollowTheRule(rule: SourceRuleTests.Rule) throws {
        let regexes = try rule.regexes()
        let files = try SourceTree.swiftFiles()
        #expect(files.isEmpty == false, "no Swift files found under \(SourceTree.scannedFolders)")
        for file in files where rule.isAllowed(file.path) == false {
            for (index, line) in file.codeLines.enumerated() {
                for regex in regexes where line.contains(regex) {
                    Issue.record(
                        "\(file.path):\(index + 1) breaks “\(rule.name)”: \(line.trimmingCharacters(in: .whitespaces))"
                    )
                }
            }
        }
    }

    @Test(arguments: rules)
    func ruleCatchesItsForbiddenExamples(rule: SourceRuleTests.Rule) throws {
        let regexes = try rule.regexes()
        for example in rule.forbiddenExamples {
            #expect(regexes.contains { example.contains($0) }, "missed: \(example)")
        }
        for example in rule.allowedExamples {
            #expect(regexes.contains { example.contains($0) } == false, "caught: \(example)")
        }
    }

    @Test(arguments: rules)
    func everyAllowedPlaceExists(rule: SourceRuleTests.Rule) {
        for path in rule.allowed {
            let exists = FileManager.default.fileExists(atPath: SourceTree.url(of: path).path())
            #expect(exists, "allowed place \(path) is missing; update the rule")
        }
    }

    // MARK: one menu item per built command

    /// The problems with how often `CommandItem(.id)` is used in the command folder and how
    /// often the app scene reads `CommandMap.entry(for: .showMainWindow)`.
    ///
    /// A built command must be used exactly once, a planned command never. A `system` row has
    /// no `CommandItem`; a built one is read exactly once in the scene file.
    ///
    /// - Parameters:
    ///   - entries: The rows to check.
    ///   - commandLines: The code lines of the command folder, without the map itself.
    ///   - sceneLines: The code lines of the app scene file.
    /// - Returns: One sentence per problem; empty when every row is used as its status says.
    nonisolated static func usageProblems(
        entries: [CommandEntry], commandLines: [String], sceneLines: [String]
    ) throws -> [String] {
        let itemUse = try Regex(#"CommandItem\(\s*\.(\w+)"#).wordBoundaryKind(.simple)
        let sceneUse = try Regex(#"CommandMap\.entry\(for:\s*\.(\w+)\)"#).wordBoundaryKind(.simple)
        func count(_ regex: Regex<AnyRegexOutput>, in lines: [String]) -> [String: Int] {
            var counts: [String: Int] = [:]
            for line in lines {
                for match in line.matches(of: regex) {
                    if let name = match.output[1].substring.map(String.init) {
                        counts[name, default: 0] += 1
                    }
                }
            }
            return counts
        }
        let items = count(itemUse, in: commandLines)
        let scene = count(sceneUse, in: sceneLines)
        var problems: [String] = []
        for entry in entries {
            let name = entry.id.rawValue
            let uses = entry.kind == .system ? scene[name, default: 0] : items[name, default: 0]
            let place = entry.kind == .system ? "the app scene" : "CommandItem"
            switch entry.status {
            case .built where uses != 1:
                problems.append("\(name) is built but used \(uses) times by \(place)")
            case .planned where uses != 0:
                problems.append("\(name) is planned but used \(uses) times by \(place)")
            default:
                break
            }
        }
        return problems
    }

    @Test func everyBuiltCommandHasOneMenuItemAndNoPlannedCommandHasAny() throws {
        let files = try SourceTree.swiftFiles()
        let commandLines =
            files
            .filter {
                $0.path.hasPrefix(Self.commandsFolder)
                    && $0.path != Self.commandsFolder + "CommandMap.swift"
            }
            .flatMap(\.codeLines)
        let sceneLines = try #require(files.first { $0.path == Self.appScene }).codeLines
        let problems = try Self.usageProblems(
            entries: CommandMap.entries, commandLines: commandLines, sceneLines: sceneLines)
        #expect(problems.isEmpty, "\(problems)")
    }

    private func entry(_ id: CommandID, status: CommandStatus) -> CommandEntry {
        CommandEntry(
            id, menu: id == .showMainWindow ? .window : .view, group: 1,
            kind: id == .showMainWindow ? .system : .button, title: "T",
            scope: .shell, owner: 69, status: status)
    }

    @Test func aBuiltCommandWithoutAMenuItemIsReported() throws {
        let problems = try Self.usageProblems(
            entries: [entry(.showOverview, status: .built)], commandLines: [], sceneLines: [])
        #expect(problems == ["showOverview is built but used 0 times by CommandItem"])
    }

    @Test func aBuiltCommandWithTwoMenuItemsIsReported() throws {
        let lines = ["CommandItem(.showOverview)", "CommandItem(.showOverview)"]
        let problems = try Self.usageProblems(
            entries: [entry(.showOverview, status: .built)], commandLines: lines, sceneLines: [])
        #expect(problems == ["showOverview is built but used 2 times by CommandItem"])
    }

    @Test func aPlannedCommandWithAMenuItemIsReported() throws {
        let problems = try Self.usageProblems(
            entries: [entry(.showOverview, status: .planned)],
            commandLines: ["CommandItem(.showOverview)"], sceneLines: [])
        #expect(problems == ["showOverview is planned but used 1 times by CommandItem"])
    }

    @Test func aBuiltCommandWithOneMenuItemPasses() throws {
        let problems = try Self.usageProblems(
            entries: [entry(.showOverview, status: .built)],
            commandLines: ["CommandItem(.showOverview)", "CommandItem(.showCatalog)"],
            sceneLines: [])
        #expect(problems.isEmpty)
    }

    // MARK: page commands on a pushed page

    /// Whether a page that publishes page commands publishes them again inside each of its
    /// `navigationDestination` closures.
    ///
    /// On macOS 27 a page pushed by a `NavigationStack` does not see the value published
    /// around the stack, so without the second publisher every page command is off in the
    /// menu while the pushed page shows (found by `InstanceCommandsUITests`, 2026-10-31).
    /// The closure is found by counting braces from the `navigationDestination` line.
    ///
    /// - Parameter lines: The code lines of one file.
    /// - Returns: The 1 based line of each `navigationDestination` whose closure publishes no
    ///   page commands; empty when the file publishes none or every closure does.
    nonisolated static func destinationsWithoutPageCommands(lines: [String]) -> [Int] {
        let publishes = ".publishesCommands(.page("
        guard lines.contains(where: { $0.contains(publishes) }) else { return [] }
        var missing: [Int] = []
        for (start, line) in lines.enumerated() where line.contains(".navigationDestination(") {
            var depth = 0
            var opened = false
            var found = false
            for inner in lines[start...] {
                if inner.contains(publishes) { found = true }
                for character in inner {
                    if character == "{" {
                        depth += 1
                        opened = true
                    } else if character == "}" {
                        depth -= 1
                    }
                }
                if opened, depth <= 0 { break }
            }
            if found == false { missing.append(start + 1) }
        }
        return missing
    }

    // covers: spec 0069 AC-11, AC-12 (detail page commands, milestone 3 fix)
    @Test func everyPushedPagePublishesThePageCommandsAgain() throws {
        let files = try SourceTree.swiftFiles()
        for file in files {
            for line in Self.destinationsWithoutPageCommands(lines: file.codeLines) {
                Issue.record(
                    "\(file.path):\(line) pushes a page that publishes no page commands")
            }
        }
    }

    @Test func aDestinationWithoutPageCommandsIsReported() {
        let lines = [
            "NavigationStack {",
            "    Table()",
            "    .navigationDestination(for: Route.self) { route in",
            "        DetailPage(id: route.id)",
            "    }",
            "}",
            ".publishesCommands(.page(.instances), commands.actions)",
        ]
        #expect(Self.destinationsWithoutPageCommands(lines: lines) == [3])
    }

    @Test func aDestinationThatPublishesAgainPasses() {
        let lines = [
            "NavigationStack {",
            "    .navigationDestination(for: Route.self) { route in",
            "        DetailPage(",
            "            back: { path.removeAll() }",
            "        )",
            "        .publishesCommands(.page(.instances), commands.actions)",
            "    }",
            "}",
            ".publishesCommands(.page(.instances), commands.actions)",
        ]
        #expect(Self.destinationsWithoutPageCommands(lines: lines).isEmpty)
    }

    @Test func aPageWithoutPageCommandsNeedsNoSecondPublisher() {
        let lines = [
            ".navigationDestination(for: Route.self) { route in",
            "    DetailPage(id: route.id)",
            "}",
        ]
        #expect(Self.destinationsWithoutPageCommands(lines: lines).isEmpty)
    }

}
