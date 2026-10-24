import AppKit
import StacklyTestSupport
import SwiftUI
import Testing

@testable import StacklyUI

/// The diff view: the kind table, and the layout (spec 0067, AC-17,
/// AC-19).
struct DiffViewTests {
    // MARK: - kind table

    @Test(
        .evidence(
            covers: ["REQ-026", "NFR-012"],
            expected: "Spec 0067 AC-17: every kind has the mark and tone of the table"))
    func everyKindHasTheMarkAndToneOfTheTable() {
        let table: [(DiffKind, String?, Tone?)] = [
            (.added, "+", .positive),
            (.removed, "−", .critical),
            (.unchanged, nil, nil),
        ]
        for (kind, mark, tone) in table {
            #expect(kind.mark == mark)
            #expect(kind.tone == tone)
        }
    }

    @Test func theRemovedMarkIsTheMinusSignAndNotAHyphen() {
        #expect(DiffKind.removed.mark == "\u{2212}")
        #expect(DiffKind.removed.mark != "-")
    }

    @Test func aLineKeepsItsPositionAsItsKey() {
        let lines = ComponentSamples.diffLines
        #expect(lines.map(\.id) == Array(0..<lines.count))
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

    @Test(
        .evidence(
            covers: ["REQ-026"],
            expected: "Spec 0067 AC-17: without lines the view shows the empty text"))
    func withoutLinesTheViewShowsTheEmptyText() {
        let empty = height(of: DiffView([], emptyText: "No differences."), width: 400)
        #expect(empty > 0)
        let longEmpty = height(
            of: DiffView([], emptyText: String(repeating: "No differences. ", count: 20)),
            width: 400)
        #expect(longEmpty > empty, "the empty text is real text that wraps")
    }
}
