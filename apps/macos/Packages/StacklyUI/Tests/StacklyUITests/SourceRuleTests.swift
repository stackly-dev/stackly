import Foundation
import Testing

/// The source rules of spec 0012 (AC-2, AC-3, AC-6, AC-15) and spec 0067 (AC-4, AC-18,
/// AC-21): forbidden patterns in
/// `StacklyUI` and the app target, each with an explicit list of allowed places.
///
/// The scan is text based. Line comments are skipped; anything else that matches,
/// including a string literal, counts.
struct SourceRuleTests {
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
    fileprivate var trimmed: String {
        trimmingCharacters(in: .whitespaces)
    }
}
