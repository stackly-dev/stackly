import AppKit
import SwiftUI
import Testing

@testable import StacklyUI

/// The checklist: the status table, the spoken row, and the two layouts (spec 0067, AC-13,
/// AC-19).
struct CheckListTests {
    // MARK: - status table

    @Test func everyStatusHasTheToneSymbolAndWordOfTheTable() {
        let table: [(CheckStatus, Tone, String?, String)] = [
            (.passed, .positive, "checkmark.circle.fill", "Passed"),
            (.warning, .caution, "exclamationmark.triangle.fill", "Warning"),
            (.failed, .critical, "xmark.circle.fill", "Failed"),
            (.pending, .neutral, nil, "Checking"),
            (.unrecognized, .caution, "questionmark.diamond.fill", "Unrecognized status"),
        ]
        for (status, tone, symbol, word) in table {
            #expect(status.tone == tone)
            #expect(status.symbol == symbol)
            #expect(status.word == word)
        }
    }

    @Test func everyStatusShapeDiffersSoColorIsNeverTheOnlySign() {
        let symbols = [CheckStatus.passed, .warning, .failed, .unrecognized].compactMap(\.symbol)
        #expect(Set(symbols).count == symbols.count)
        #expect(!symbols.contains { $0.isEmpty })
    }

    // MARK: - spoken row

    @Test func aRowIsReadAsTitleStatusAndDetail() {
        let item = CheckItem(
            id: "disk", status: .warning, title: "Low disk space", detail: "4.1 GB is left.")
        #expect(item.accessibilityLabel == "Low disk space, Warning: 4.1 GB is left.")
    }

    @Test func aRowWithoutADetailIsReadAsTitleAndStatus() {
        #expect(
            CheckItem(id: "port", status: .passed, title: "Port 5432 is free").accessibilityLabel
                == "Port 5432 is free, Passed")
        #expect(
            CheckItem(id: "port", status: .pending, title: "Checking", detail: "")
                .accessibilityLabel == "Checking, Checking")
    }

    @Test func theRowIdentifierUsesTheItemKey() {
        #expect(AccessibilityID.Checklist.row("disk") == "checklist.row.disk")
    }

    // MARK: - layout

    /// The height a view takes at `width`.
    private func height(of view: some View, width: CGFloat) -> CGFloat {
        let host = NSHostingView(rootView: view.frame(width: width))
        host.frame = CGRect(x: 0, y: 0, width: width, height: 10)
        host.layoutSubtreeIfNeeded()
        return host.fittingSize.height
    }

    private let items = (1...4).map {
        CheckItem(id: "c\($0)", status: .passed, title: "Check number \($0)")
    }

    @Test func theAdaptiveLayoutUsesTwoColumnsInASheet() {
        let adaptive = height(of: CheckList(items), width: 488)
        let single = height(of: CheckList(items, layout: .single), width: 488)
        #expect(adaptive > 0)
        #expect(adaptive < single, "two columns are shorter than one")
    }

    @Test func theAdaptiveLayoutFallsBackToOneColumnWhenNarrow() {
        let adaptive = height(of: CheckList(items), width: 300)
        let single = height(of: CheckList(items, layout: .single), width: 300)
        #expect(adaptive == single, "a column is at least 200 pt, so 300 pt holds one")
    }

    @Test func aLongDetailWrapsInFullInsteadOfBeingCut() {
        let long = CheckItem(
            id: "long", status: .warning, title: "Title",
            detail: String(repeating: "word ", count: 40))
        let short = CheckItem(id: "short", status: .warning, title: "Title", detail: "word")
        #expect(
            height(of: CheckList([long], layout: .single), width: 300)
                > height(of: CheckList([short], layout: .single), width: 300) * 2)
    }

    @Test func aRowActionAddsAButtonUnderTheText() {
        let plain = CheckItem(id: "a", status: .failed, title: "Folder", detail: "Read only.")
        let withAction = CheckItem(
            id: "a", status: .failed, title: "Folder", detail: "Read only.",
            action: StacklyAction(
                id: "choose", title: "Choose Folder…", mutates: false, perform: {})
        )
        #expect(
            height(of: CheckList([withAction], layout: .single), width: 400)
                > height(of: CheckList([plain], layout: .single), width: 400))
    }
}
