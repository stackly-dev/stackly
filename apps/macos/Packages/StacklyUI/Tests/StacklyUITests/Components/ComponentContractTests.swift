import Foundation
import StacklyTestSupport
import Testing

@testable import StacklyUI

/// The contract of the shared components that the per component suites do not reach
/// (spec 0067, AC-1, AC-2, AC-19, AC-21): where the components live, that no screen has a
/// second version, what the tile draws, the design.md section, the accessibility source
/// rules, and the edge of the `.secondary` source rule that the developer accepted.
///
/// A hosted view builds no accessibility tree without a running assistive client, so most
/// checks read the source, as the other component suites do.
///
/// `nonisolated` and serialized like `SourceRuleTests`, so the source scans stay off the
/// main actor; a `nonisolated` test inside a main actor suite would still run on it.
@Suite(.serialized) nonisolated struct ComponentContractTests {
    private static let sources = "apps/macos/Packages/StacklyUI/Sources/StacklyUI/"

    private func code(_ path: String) throws -> String {
        SourceFile(
            path: path, text: try String(contentsOf: SourceTree.url(of: path), encoding: .utf8)
        )
        .codeLines.joined(separator: "\n")
    }

    /// The code of every Swift file under `Components/`, by path.
    private func componentFiles() throws -> [SourceFile] {
        try SourceTree.swiftFiles().filter { $0.path.hasPrefix(Self.sources + "Components/") }
    }

    // MARK: - AC-1: location, names, and no second version

    /// The types of the spec's *Signature Swift* section that are declared in `Components/`.
    /// Only top level declarations count, so `AccessibilityID.Feedback` is not a second
    /// `Feedback`.
    private static let componentTypes = [
        "ProviderTile", "TileSpec", "ProviderMark", "CardSection", "CopyButton",
        "CopyConfirmation", "PlainPasteboard", "ConsoleBlock", "ConsoleCopy", "StacklyAction",
        "ActionRole", "ActionConfirmation", "ActionAvailability", "ActionHost", "ActionButton",
        "ActionMenu", "ActionMenuItems", "Feedback", "FeedbackCenter", "FeedbackHold",
        "FeedbackCard", "StacklySheet", "CheckStatus", "CheckItem", "CheckList",
        "StageDefinition", "StagedProgressValue", "StagedProgress", "LogLine", "LogView",
        "DiffLine", "DiffView", "InspectorToggle", "InspectorHeader", "InspectorPlaceholder",
        "StacklyInspectorWidth",
    ]

    @Test(
        .evidence(
            covers: ["REQ-024"],
            expected:
                "Spec 0067 AC-1: every component type is declared once, under Components/, "
                + "and nowhere else in StacklyUI"
        ))
    func everyComponentTypeIsDeclaredOnceAndOnlyUnderComponents() throws {
        let files = try SourceTree.swiftFiles().filter { $0.path.hasPrefix(Self.sources) }
        for name in Self.componentTypes {
            let declaration = try Regex(
                #"^(?:(?:public|internal|package|final|nonisolated)\s+)*(?:struct|enum|class|actor)\s+"#
                    + name + #"\b"#
            ).wordBoundaryKind(.simple)
            let declaredIn = files.filter { file in
                file.codeLines.contains { $0.contains(declaration) }
            }.map(\.path)
            #expect(declaredIn.count == 1, "\(name) is declared in \(declaredIn)")
            for path in declaredIn {
                #expect(
                    path.hasPrefix(Self.sources + "Components/"),
                    "\(name) lives in \(path), not in Components/")
            }
        }
    }

    @Test func theFoldersOfComponentsAreTheOnesTheSpecNames() throws {
        let folders = Set(
            try componentFiles().map { file in
                String(
                    file.path.dropFirst((Self.sources + "Components/").count)
                        .split(separator: "/").first ?? "")
            })
        #expect(
            folders
                == [
                    "Tile", "KeyValue", "Copy", "Actions", "Feedback", "Sheet", "Checklist",
                    "Progress", "Log", "Diff", "Inspector",
                ])
    }

    @Test func noComponentWritesItsOwnPasteboardOrDialog() throws {
        for file in try componentFiles() {
            for (index, line) in file.codeLines.enumerated() {
                for forbidden in ["NSPasteboard", "confirmationDialog(", ".alert(", "NSAlert"]
                where file.path.hasSuffix("Copy/PlainPasteboard.swift") == false {
                    #expect(
                        line.contains(forbidden) == false,
                        "\(file.path):\(index + 1) draws its own \(forbidden)")
                }
            }
        }
    }

    // MARK: - AC-1: design.md

    private func designMarkdown() throws -> String {
        try String(contentsOf: SourceTree.url(of: "apps/macos/design.md"), encoding: .utf8)
    }

    @Test(
        .evidence(
            covers: ["REQ-024"],
            expected:
                "Spec 0067 AC-1: design.md has a Components section with one subsection per "
                + "component family and the checklist and staged progress tables"
        ))
    func designMdHasAComponentsSectionWithEveryFamilyAndBothTables() throws {
        let design = try designMarkdown()
        let components = try #require(design.range(of: "\n## Components\n"))
        let next = try #require(
            design.range(of: "\n## Motion\n", range: components.upperBound..<design.endIndex))
        let section = String(design[components.upperBound..<next.lowerBound])
        for heading in [
            "Tile", "Key value", "Copy and console", "Actions", "Menus", "Feedback", "Sheet",
            "Checklist", "Staged progress", "Inspector", "Log", "Diff",
        ] {
            #expect(section.contains("\n### \(heading)\n"), "missing ### \(heading)")
        }
        // The checklist table and the staged progress state table.

    }

    @Test func designMdStatusTableMatchesTheCheckStatusCode() throws {
        let design = try designMarkdown()
        let statuses: [(name: String, symbol: String?)] = [
            ("passed", CheckStatus.passed.symbol),
            ("warning", CheckStatus.warning.symbol),
            ("failed", CheckStatus.failed.symbol),
            ("pending", CheckStatus.pending.symbol),
            ("unrecognized", CheckStatus.unrecognized.symbol),
        ]
        for status in statuses {
            let row = try #require(
                design.split(separator: "\n").first { $0.hasPrefix("| `\(status.name)` |") },
                "no row for \(status.name)")
            if let symbol = status.symbol {
                #expect(row.contains("`\(symbol)`"), "symbol of \(status.name)")
            }
        }
    }

    // MARK: - AC-2: what the tile draws

    @Test func theTileMarkFontIsSizedByTheTypographyHelper() throws {
        let typography = try code(Self.sources + "Tokens/Typography.swift")
        #expect(typography.contains("func stacklyTileMark(tileSize:"))
    }

    @MainActor @Test func markTextOfAWhitespaceMarkKeepsTheCatalogMark() {
        // The catalog mark wins whenever it is not empty, even when it is only a space.
        let tile = ProviderTile(mark: " ", tile: .blue, name: "Valkey", size: 40)
        #expect(tile.markText == " ")
    }

    @MainActor @Test func aTileWithoutAMarkOrANameShowsAQuestionMarkOnGray() {
        let tile = ProviderTile(TileSpec(mark: nil, tile: .unrecognized), size: 26)
        #expect(tile.markText == "?")
        #expect(TileFill.color(for: .unrecognized) == TileFill.color(for: nil))
    }

    // MARK: - AC-20: previews, samples, and the gallery page

    @Test(
        .evidence(
            covers: ["REQ-024"],
            expected:
                "Spec 0067 AC-20: every component view has Light, Dark, and Narrow width "
                + "previews, and the samples and the gallery page exist only in Debug"
        ))
    func everyComponentViewHasThreePreviewsAndTheSamplesExistOnlyInDebug() throws {
        let view = #/^\s*(?:public\s+)?struct\s+\w+(?:<[^>]*>)?\s*:\s*View\b/#
        for file in try componentFiles() {
            guard file.codeLines.contains(where: { $0.firstMatch(of: view) != nil }) else {
                continue
            }
            let text = file.codeLines.joined(separator: "\n")
            for name in ["Light", "Dark", "Narrow width"] {
                #expect(
                    text.contains("#Preview(\"\(name)\")"), "\(file.path) has no \(name) preview")
            }
        }
        let gallery = try SourceTree.swiftFiles().filter {
            $0.path.hasPrefix(Self.sources + "Gallery/")
                && ($0.path.contains("ComponentSamples") || $0.path.contains("ComponentsGallery"))
        }
        #expect(gallery.count >= 4, "the sample and gallery files moved")
        for file in gallery {
            #expect(file.startsWithIfDebug, "\(file.path) must start with #if DEBUG")
        }
    }

    @Test func theComponentsGalleryPageHasASectionGroupForEveryFamily() throws {
        let page = try code(Self.sources + "Gallery/ComponentsGalleryPage.swift")
        for group in [
            "BaseComponentsSections()", "SheetComponentsSections()",
            "InspectorComponentsSections()", "LogComponentsSections()",
        ] {
            #expect(page.contains(group), "\(group) is not on the page")
        }
    }

    // MARK: - AC-19: accessibility source rules

    @Test(
        .evidence(
            covers: ["NFR-012", "AC-027"],
            expected:
                "Spec 0067 AC-19: no component paints meaning with StacklyColor.decorative and "
                + "every custom click area is at least 24 x 24 pt"
        ))
    func noComponentUsesTheDecorativeColorAndCustomClickAreasAreAtLeast24Points() throws {
        for file in try componentFiles() {
            for (index, line) in file.codeLines.enumerated() {
                #expect(
                    line.contains("StacklyColor.decorative") == false
                        && line.contains(".decorative") == false,
                    "\(file.path):\(index + 1) uses the decorative color")
            }
        }
        let dismiss = try code(Self.sources + "Components/Feedback/FeedbackCard.swift")
        #expect(
            dismiss.contains(".frame(minWidth: HitTarget.minimum, minHeight: HitTarget.minimum)"))

        #expect(HitTarget.minimum >= 24)
    }

    @MainActor @Test func theInspectorToggleIsLabelledByState() throws {

        #expect(InspectorToggle.label(isPresented: false) == "Show Inspector")
        #expect(InspectorToggle.label(isPresented: true) == "Hide Inspector")
    }

    // MARK: - AC-21: the source rules and the `.secondary` edge

    private func secondaryRule() throws -> SourceRuleTests.Rule {
        try #require(
            SourceRuleTests.rules.first { $0.name.hasPrefix("secondary, tertiary") },
            "the secondary rule moved or was renamed")
    }

    private func isCaught(_ line: String, by rule: SourceRuleTests.Rule) throws -> Bool {
        try rule.regexes().contains { line.contains($0) }
    }

    @Test(
        .evidence(
            covers: ["REQ-024"],
            expected:
                "Spec 0067 AC-21: a line that sets prominence .secondary is accepted even when a "
                + "color .secondary follows on the same line, and a color before it is caught"
        ))
    func aColorSecondaryAfterAProminenceOnTheSameLineIsAcceptedAndOneBeforeItIsCaught() throws {
        let rule = try secondaryRule()
        // Accepted on purpose: the rule skips the rest of a line once it names a prominence.
        #expect(
            try isCaught(
                "ActionButton(a, prominence: .secondary).foregroundStyle(.secondary)", by: rule)
                == false)
        #expect(
            try isCaught(
                "init(prominence: ActionProminence = .secondary, tint: .secondary)", by: rule)
                == false)
        // A color first is still caught, even when a prominence follows it.
        #expect(
            try isCaught(
                ".foregroundStyle(.secondary); ActionButton(a, prominence: .secondary)", by: rule))
        // The prominence on one line does not hide a color on the next one.
        #expect(try isCaught("    .foregroundStyle(.secondary)", by: rule))
    }

    @Test func theSecondaryRuleStillAllowsThePlacementAndTheTwoProminenceSpellings() throws {
        let rule = try secondaryRule()
        #expect(try isCaught("ToolbarItem(placement: .secondaryAction) {}", by: rule) == false)
        #expect(try isCaught("prominence: .secondary", by: rule) == false)
        #expect(try isCaught("prominence: ActionProminence = .secondary", by: rule) == false)
        #expect(try isCaught("prominence:.secondary", by: rule), "the spelling needs a space")
    }

    @Test func theComponentsFolderHasNoLineTheSecondaryRuleCatches() throws {
        let rule = try secondaryRule()
        for file in try componentFiles() {
            for (index, line) in file.codeLines.enumerated() {
                #expect(
                    try isCaught(line, by: rule) == false,
                    "\(file.path):\(index + 1) uses a raw secondary color: \(line.trimmingCharacters(in: .whitespaces))"
                )
            }
        }
    }

    @Test func theTwoNewRulesExistAndAllowOnlyTheirOwnFolders() throws {
        let pasteboard = try #require(
            SourceRuleTests.rules.first { $0.name.contains("pasteboard") })
        #expect(
            pasteboard.allowed == [
                Self.sources + "Components/Copy/PlainPasteboard.swift",
                Self.sources + "Secrets/ConcealedPasteboard.swift",
            ])
        let inspector = try #require(SourceRuleTests.rules.first { $0.name.contains("inspector") })
        #expect(inspector.allowed == [Self.sources + "Components/Inspector/"])
    }
}
