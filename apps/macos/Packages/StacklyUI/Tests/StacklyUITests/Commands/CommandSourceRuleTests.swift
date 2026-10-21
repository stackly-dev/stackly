import Foundation
import Testing

@testable import StacklyUI

/// The source rules of spec 0069 (AC-1): a shortcut and a menu live only in the command
/// folder and the app's scene file, and every built command has exactly one menu item.
///
/// This suite reuses the `Rule` type and the source scan of `SourceRuleTests`, so the scan
/// is text based and skips line comments.
struct CommandSourceRuleTests {
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

    @Test func aSystemCommandIsUsedByTheSceneNotByAMenuItem() throws {
        let scene = ["    .keyboardShortcut(CommandMap.entry(for: .showMainWindow).shortcut)"]
        let built = entry(.showMainWindow, status: .built)
        #expect(
            try Self.usageProblems(entries: [built], commandLines: [], sceneLines: scene).isEmpty)
        #expect(
            try Self.usageProblems(entries: [built], commandLines: [], sceneLines: []).count == 1)
        #expect(
            try Self.usageProblems(
                entries: [built], commandLines: ["CommandItem(.showMainWindow)"],
                sceneLines: scene
            ).isEmpty)
        let planned = entry(.showMainWindow, status: .planned)
        #expect(
            try Self.usageProblems(entries: [planned], commandLines: [], sceneLines: scene).count
                == 1)
    }
}
