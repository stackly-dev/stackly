import Foundation
import Testing

/// The source rules of spec 0012 (AC-2, AC-3, AC-6, AC-15) and spec 0067 (AC-4, AC-18,
/// AC-21): forbidden patterns in
/// `StacklyUI` and the app target, each with an explicit list of allowed places.
///
/// The scan is text based. Line comments are skipped; anything else that matches,
/// including a string literal, counts.
///
/// The suite is `nonisolated` and serialized: its regex scans take seconds of CPU, which on
/// the main actor would hold every other test of a full run, and one test at a time keeps
/// them from filling the cooperative pool.
@Suite(.serialized) nonisolated struct SourceRuleTests {
    /// One forbidden pattern family and the only places that may use it.
    nonisolated struct Rule: CustomTestStringConvertible, Sendable {
        /// What the rule forbids, with the acceptance criterion it guards.
        let name: String
        /// Regular expressions; a code line matching any of them is a violation.
        let patterns: [String]
        /// Files, or folders ending in `/`, relative to the repository root, where the
        /// patterns are allowed.
        let allowed: [String]
        /// Code the rule must catch, so a broken pattern cannot pass silently.
        let forbiddenExamples: [String]
        /// Code the rule must not catch.
        let allowedExamples: [String]

        var testDescription: String { name }

        /// The compiled patterns. `\b` uses simple word boundaries: the Unicode default
        /// treats `Font.system` as one word, so `\bsystem` would miss it.
        func regexes() throws -> [Regex<AnyRegexOutput>] {
            try patterns.map { try Regex($0).wordBoundaryKind(.simple) }
        }

        func isAllowed(_ path: String) -> Bool {
            allowed.contains { $0.hasSuffix("/") ? path.hasPrefix($0) : path == $0 }
        }
    }

    nonisolated static let stacklyUI = "apps/macos/Packages/StacklyUI/Sources/StacklyUI/"

    nonisolated static let rules = [
        Rule(
            name: "fixed font size outside the symbol helper (AC-2)",
            patterns: [#"\bsystem\(\s*size:"#, #"\.custom\("#, #"\bNSFont\b"#],
            allowed: [stacklyUI + "Tokens/Typography.swift"],
            forbiddenExamples: [
                ".font(.system(size: 13))",
                "Text(\"a\").font(Font.system(size: 12, weight: .bold))",
                ".font(.custom(\"Inter\", size: 13))",
                "let font = NSFont(name: \"Menlo\", size: 12)",
            ],
            allowedExamples: [".font(.body)", ".font(.system(.body, design: .monospaced))"]
        ),
        Rule(
            name: "secondary, tertiary, or accent text color outside the token file (AC-3)",
            patterns: [
                // Every `.secondary` is a color or style except the prominence of an action
                // button, which is written `prominence: .secondary` or
                // `prominence: ActionProminence = .secondary`. Swift regex has no lookbehind,
                // so the scan from the line start refuses to step over that spelling.
                #"^(?:(?!prominence: (?:ActionProminence = )?\.secondary).)*?\.secondary\b"#,
                #"\.tertiary\b"#,
                #"\bsecondaryLabelColor\b"#,
                #"\btertiaryLabelColor\b"#,
                #"\baccentColor\b"#,
                #"\.tint\b(?!\s*\()"#,
            ],
            allowed: [stacklyUI + "Tokens/StacklyColor.swift"],
            forbiddenExamples: [
                ".foregroundStyle(.secondary)",
                ".foregroundStyle(.tertiary)",
                "Color(nsColor: .secondaryLabelColor)",
                "Color(nsColor: .tertiaryLabelColor)",
                ".foregroundStyle(Color.accentColor)",
                ".foregroundStyle(.tint)",
                "Color.secondary",
                ".foregroundStyle(isOn ? .secondary : .primary)",
                "let color: Color = .secondary",
                "label(style: .secondary)",
                "cond ? .primary : .secondary",
                ".foregroundStyle(.red, .secondary)",
                "return .secondary",
                "[.secondary]",
                "{ .secondary }",
                "static let a: HierarchicalShapeStyle = .secondary",
            ],
            allowedExamples: [
                "ActionButton(action, prominence: .secondary)",
                "prominence: ActionProminence = .secondary,",
                ".foregroundStyle(StacklyColor.textSecondary)",
                ".tint(.red)",
                "ToolbarItem(placement: .secondaryAction) {}",
            ]
        ),
        Rule(
            name: "custom glass or material (AC-6)",
            patterns: [
                #"\.glassEffect"#,
                #"\bGlassEffectContainer\b"#,
                #"\.buttonStyle\(\s*\.glass"#,
                #"\bMaterial\b"#,
                #"\b(ultraThin|thin|regular|thick|ultraThick)Material\b"#,
                #"\.bar\b"#,
            ],
            allowed: [],
            forbiddenExamples: [
                ".glassEffect(.regular, in: .capsule)",
                "GlassEffectContainer { content }",
                ".buttonStyle(.glass)",
                ".buttonStyle(.glassProminent)",
                ".background(.ultraThinMaterial)",
                ".background(Material.regular)",
                ".background(.bar)",
            ],
            allowedExamples: [".buttonStyle(.borderedProminent)", ".toolbar { }"]
        ),
        Rule(
            name: "dialog outside the Confirmation folder (AC-15)",
            patterns: [#"\bconfirmationDialog\("#, #"\.alert\("#, #"\bNSAlert\b"#],
            allowed: [stacklyUI + "Confirmation/"],
            forbiddenExamples: [
                ".confirmationDialog(\"Remove?\", isPresented: $shown) {}",
                ".alert(\"Failed\", isPresented: $shown) {}",
                "let alert = NSAlert()",
            ],
            allowedExamples: [".stacklyConfirmation(item: $request)"]
        ),
        Rule(
            name: "pasteboard outside the copy helpers (spec 0067 AC-4)",
            patterns: [#"\bNSPasteboard\b"#],
            allowed: [
                stacklyUI + "Components/Copy/PlainPasteboard.swift",
                stacklyUI + "Secrets/ConcealedPasteboard.swift",
            ],
            forbiddenExamples: [
                "NSPasteboard.general.clearContents()",
                "let pasteboard: NSPasteboard = .general",
            ],
            allowedExamples: [
                "PlainPasteboard.write(text)",
                "ConcealedPasteboard.shared.copy(secret)",
            ]
        ),
        Rule(
            name: "underscored environment setter outside FixtureAppearance (spec 0068 AC-7)",
            patterns: [#"\b_colorSchemeContrast\b"#, #"\b_accessibility[A-Za-z]*"#],
            allowed: [stacklyUI + "Fixtures/FixtureAppearance.swift"],
            forbiddenExamples: [
                ".environment(\\._colorSchemeContrast, .increased)",
                ".environment(\\._accessibilityReduceTransparency, true)",
                "env._accessibilityDifferentiateWithoutColor = true",
                ".environment(\\._accessibilityInvertColors, true)",
            ],
            allowedExamples: [
                ".environment(\\.accessibilityReduceTransparency, false)",
                "@Environment(\\.colorSchemeContrast) private var contrast",
                "accessibilityLabel(\"a\")",
            ]
        ),
        Rule(
            name: "fixture scenario variables outside the app delegate (spec 0068 AC-7)",
            patterns: [#"\bSTACKLY_UI_SCENARIO\b"#, #"\bSTACKLY_UI_APPEARANCE\b"#],
            allowed: ["apps/macos/Stackly/AppDelegate.swift"],
            forbiddenExamples: [
                "environment[\"STACKLY_UI_SCENARIO\"]",
                "let name = \"STACKLY_UI_APPEARANCE\"",
            ],
            allowedExamples: ["environment[\"STACKLY_ROOT\"]"]
        ),
        Rule(
            name: "ServiceManagement outside the SMAppService adapter (spec 0015 AC-11)",
            patterns: [#"\bimport ServiceManagement\b"#, #"\bSMAppService\b"#],
            allowed: [stacklyUI + "Agent/SMAppServiceAgent.swift"],
            forbiddenExamples: [
                "import ServiceManagement",
                "let service = SMAppService.mainApp",
                "SMAppService.openSystemSettingsLoginItems()",
            ],
            allowedExamples: [
                "let agent = SMAppServiceAgent()",
                "controller.openApprovalSettings()",
            ]
        ),
        Rule(
            name: "registering or relaunching the agent outside the Agent folder (spec 0015 AC-11)",
            patterns: [#"\.(register|unregister|relaunch)\(\)"#],
            allowed: [stacklyUI + "Agent/"],
            forbiddenExamples: [
                "try await controller.register()",
                "_ = try await agent.relaunch()",
                "service.unregister()",
            ],
            allowedExamples: ["sync.register(store)", "await agent.turnOn()", "model.start()"]
        ),
        Rule(
            name: "file changes outside the command line tool (spec 0015 AC-14)",
            patterns: [
                #"\bcreateSymbolicLink\("#, #"\bcreateDirectory\("#, #"\bremoveItem\("#,
                #"\bmoveItem\("#, #"\breplaceItem"#, #"\brename\("#,
            ],
            allowed: [stacklyUI + "Agent/CommandLineTool.swift"],
            forbiddenExamples: [
                "try manager.createSymbolicLink(at: link, withDestinationURL: target)",
                "try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)",
                "try FileManager.default.removeItem(at: url)",
                "rename(from, to)",
            ],
            allowedExamples: [
                "FileManager.default.fileExists(atPath: path)",
                "let renamed = title",
            ]
        ),
        Rule(
            name: "inspector outside the inspector components (spec 0067 AC-18)",
            patterns: [#"\.inspector\("#],
            allowed: [stacklyUI + "Components/Inspector/"],
            forbiddenExamples: [
                ".inspector(isPresented: $shown) { Text(\"a\") }",
                "content.inspector(isPresented: binding) {}",
            ],
            allowedExamples: [
                ".stacklyInspector(isPresented: $shown) {}",
                "InspectorToggle(isPresented: $shown)",
            ]
        ),
    ]

    @Test(arguments: rules)
    func sourcesFollowTheRule(rule: Rule) throws {
        let regexes = try rule.regexes()
        let files = try SourceTree.swiftFiles()
        #expect(files.isEmpty == false, "no Swift files found under \(SourceTree.scannedFolders)")
        for file in files where rule.isAllowed(file.path) == false {
            for (index, line) in file.codeLines.enumerated() {
                for regex in regexes where line.contains(regex) {
                    Issue.record(
                        "\(file.path):\(index + 1) breaks “\(rule.name)”: \(line.trimmed)"
                    )
                }
            }
        }
    }

    @Test(arguments: rules)
    func ruleCatchesItsForbiddenExamples(rule: Rule) throws {
        let regexes = try rule.regexes()
        for example in rule.forbiddenExamples {
            #expect(regexes.contains { example.contains($0) }, "missed: \(example)")
        }
        for example in rule.allowedExamples {
            #expect(regexes.contains { example.contains($0) } == false, "caught: \(example)")
        }
    }

    @Test(arguments: rules)
    func everyAllowedPlaceExists(rule: Rule) {
        for path in rule.allowed where path.hasSuffix("/") == false {
            #expect(
                FileManager.default.fileExists(atPath: SourceTree.url(of: path).path()),
                "allowed file \(path) is missing; update the rule"
            )
        }
    }

    @Test func everyFixtureFileStartsWithIfDebug() throws {
        let fixtures = try SourceTree.swiftFiles().filter {
            $0.path.hasPrefix(Self.stacklyUI + "Fixtures/")
        }
        #expect(fixtures.isEmpty == false, "no files under Fixtures/")
        for file in fixtures {
            #expect(
                file.startsWithIfDebug,
                "\(file.path) must start with #if DEBUG so it never reaches a Release build"
            )
        }
    }

    @Test func theIfDebugCheckSeesTheFirstCodeLine() {
        #expect(SourceFile(path: "a", text: "#if DEBUG\nimport A\n#endif\n").startsWithIfDebug)
        #expect(SourceFile(path: "a", text: "// c\n\n#if DEBUG\n#endif\n").startsWithIfDebug)
        #expect(
            SourceFile(path: "a", text: "import A\n#if DEBUG\n#endif\n").startsWithIfDebug == false)
        #expect(SourceFile(path: "a", text: "#if os(macOS)\n#endif\n").startsWithIfDebug == false)
    }

    @Test func scenarioVariablesAreReadOnlyInsideIfDebug() throws {
        let delegate = try SourceTree.swiftFiles().filter {
            $0.path == "apps/macos/Stackly/AppDelegate.swift"
        }
        #expect(delegate.count == 1)
        for file in delegate {
            for number in file.lineNumbers(matching: "STACKLY_UI_", outsideIfDebug: true) {
                Issue.record("\(file.path):\(number) reads a fixture variable outside #if DEBUG")
            }
        }
    }

    @Test func theIfDebugScanKnowsNestingAndElse() {
        let text = """
            let a = "STACKLY_UI_X"
            #if DEBUG
            let b = "STACKLY_UI_X"
            #if os(macOS)
            let c = "STACKLY_UI_X"
            #endif
            #else
            let d = "STACKLY_UI_X"
            #endif
            let e = "STACKLY_UI_X"
            """
        let file = SourceFile(path: "a", text: text)
        #expect(file.lineNumbers(matching: "STACKLY_UI_", outsideIfDebug: true) == [1, 8, 10])
    }

    // MARK: - Every page has a snapshot suite (spec 0068 AC-7)

    /// The names of the top level `struct <Name>Page: View` declarations in `lines` that are
    /// not `private` or `fileprivate`.
    nonisolated static func pageNames(in lines: [String]) -> [String] {
        let declaration =
            #/^\s*(?:(?:public|internal|package|final)\s+)*struct\s+(\w+Page)\s*:\s*View\b/#
        return lines.compactMap { line in
            line.firstMatch(of: declaration).map { String($0.output.1) }
        }
    }

    /// The pages in `pages` that no line of `testLines` names as a whole word.
    nonisolated static func unsnapshotted(pages: [String], testLines: [String]) throws -> [String] {
        try pages.filter { page in
            let word = try Regex("\\b\(page)\\b").wordBoundaryKind(.simple)
            return testLines.contains { $0.contains(word) } == false
        }
    }

    @Test func everyPageOutsideTheGalleryHasASnapshotSuite() throws {
        let sources = try SourceTree.swiftFiles().filter {
            $0.path.hasPrefix(Self.stacklyUI)
                && $0.path.hasPrefix(Self.stacklyUI + "Gallery/") == false
        }
        let pages = sources.flatMap { Self.pageNames(in: $0.codeLines) }
        #expect(pages.isEmpty == false, "no page found; the scan is broken")
        let testFolder = "apps/macos/Packages/StacklyUI/Tests/StacklyUITests/SnapshotTests"
        let testLines = try SourceTree.files(under: testFolder)
            .filter { $0.hasSuffix(".swift") }
            .flatMap { path in
                SourceFile(
                    path: path,
                    text: try String(contentsOf: SourceTree.url(of: path), encoding: .utf8)
                ).codeLines
            }
        for page in try Self.unsnapshotted(pages: pages, testLines: testLines) {
            Issue.record("\(page) has no snapshot suite: name it in a file under \(testFolder)")
        }
    }

    @Test func thePageScanSkipsPrivatePagesAndNamesOnlyWholeWords() throws {
        let lines = [
            "public struct UnbuiltPage: View {",
            "private struct UnbuiltScrollPage: View {",
            "    fileprivate struct HiddenPage: View {",
            "struct DiagnosticsPage: View {",
            "final class NotAPage: View {",
            "struct Overview: View {",
            "struct PageHelper: View {",
        ]
        #expect(Self.pageNames(in: lines) == ["UnbuiltPage", "DiagnosticsPage"])
        let tests = ["UnbuiltPage(section: .overview)", "DiagnosticsPageHelper()"]
        #expect(
            try Self.unsnapshotted(pages: ["UnbuiltPage", "DiagnosticsPage"], testLines: tests)
                == ["DiagnosticsPage"])
    }

    @Test func lineCommentsAreSkippedButStringsAreKept() {
        #expect(SourceFile.removingLineComment(from: "a // .secondary") == "a ")
        #expect(SourceFile.removingLineComment(from: "/// doc .secondary") == "")
        #expect(
            SourceFile.removingLineComment(from: #"let url = "https://x" // c"#)
                == #"let url = "https://x" "#
        )
        #expect(
            SourceFile.removingLineComment(from: #"let s = "a\"//b""#) == #"let s = "a\"//b""#
        )
    }

    @Test func accentColorAssetHasNoColorValue() throws {
        let path = "apps/macos/Stackly/Assets.xcassets/AccentColor.colorset/Contents.json"
        let data = try Data(contentsOf: SourceTree.url(of: path))
        let json = try #require(try JSONSerialization.jsonObject(with: data) as? [String: Any])
        let colors = try #require(json["colors"] as? [[String: Any]])
        #expect(colors.allSatisfy { $0["color"] == nil }, "AccentColor must stay empty")
    }

    @Test func noCustomColorLivesInAnAssetCatalog() throws {
        let colorSets = try SourceTree.files(under: "apps/macos")
            .filter { $0.contains(".colorset/") && $0.contains("/AccentColor.colorset/") == false }
        #expect(colorSets.isEmpty, "custom colors belong in StacklyUI tokens: \(colorSets)")
    }
}

extension String {
    /// The string without leading and trailing white space.
    fileprivate nonisolated var trimmed: String {
        trimmingCharacters(in: .whitespaces)
    }
}
