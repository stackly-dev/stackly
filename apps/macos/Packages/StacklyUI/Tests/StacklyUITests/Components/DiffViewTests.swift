import AppKit
import SwiftUI
import Testing

@testable import StacklyUI

/// The diff view: the kind table, the spoken line, and the layout (spec 0067, AC-17,
/// AC-19).
struct DiffViewTests {
    // MARK: - kind table

    @Test func everyKindHasTheMarkToneAndWordOfTheTable() {
        let table: [(DiffKind, String?, Tone?, String)] = [
            (.added, "+", .positive, "Added"),
            (.removed, "−", .critical, "Removed"),
            (.unchanged, nil, nil, "Unchanged"),
        ]
        for (kind, mark, tone, word) in table {
            #expect(kind.mark == mark)
            #expect(kind.tone == tone)
            #expect(kind.word == word)
        }
    }

    @Test func theRemovedMarkIsTheMinusSignAndNotAHyphen() {
        #expect(DiffKind.removed.mark == "\u{2212}")
        #expect(DiffKind.removed.mark != "-")
    }

    // MARK: - spoken line

    @Test func aLineIsReadAsItsKindAndText() {
        #expect(
            DiffLine(id: 0, kind: .added, text: "port = 5433").accessibilityLabel
                == "Added: port = 5433")
        #expect(
            DiffLine(id: 1, kind: .removed, text: "port = 5432").accessibilityLabel
                == "Removed: port = 5432")
        #expect(
            DiffLine(id: 2, kind: .unchanged, text: "host = 'localhost'").accessibilityLabel
                == "Unchanged: host = 'localhost'")
    }

    @Test func aLineKeepsItsPositionAsItsKey() {
        let lines = ComponentSamples.diffLines
        #expect(lines.map(\.id) == Array(0..<lines.count))
    }

    @Test func theListIdentifierIsFixed() {
        #expect(AccessibilityID.Diff.list == "diff.list")
    }

    // MARK: - layout

    /// The height a view takes at `width`.
    private func height(of view: some View, width: CGFloat) -> CGFloat {
        let host = NSHostingView(rootView: view.frame(width: width))
        host.frame = CGRect(x: 0, y: 0, width: width, height: 10)
        host.layoutSubtreeIfNeeded()
        return host.fittingSize.height
    }

    @Test func aLongLineWrapsInsteadOfBeingCut() {
        let short = [DiffLine(id: 0, kind: .added, text: "ok")]
        let long = [DiffLine(id: 0, kind: .added, text: String(repeating: "word ", count: 60))]
        #expect(
            height(of: DiffView(long, emptyText: "none"), width: 400)
                > height(of: DiffView(short, emptyText: "none"), width: 400) * 2)
    }

    @Test func moreLinesTakeMoreHeight() {
        let one = Array(ComponentSamples.diffLines.prefix(1))
        #expect(
            height(of: DiffView(ComponentSamples.diffLines, emptyText: "none"), width: 400)
                > height(of: DiffView(one, emptyText: "none"), width: 400))
    }

    @Test func withoutLinesTheViewShowsTheEmptyText() {
        let empty = height(of: DiffView([], emptyText: "No differences."), width: 400)
        #expect(empty > 0)
        let longEmpty = height(
            of: DiffView([], emptyText: String(repeating: "No differences. ", count: 20)),
            width: 400)
        #expect(longEmpty > empty, "the empty text is real text that wraps")
    }
}
