import AppKit
import StacklyTestSupport
import SwiftUI
import Testing

@testable import StacklyUI

/// The checklist: the status table, and the two layouts (spec 0067, AC-13,
/// AC-19).
struct CheckListTests {
    // MARK: - status table

    @Test(
        .evidence(
            covers: ["NFR-012"],
            expected: "Spec 0067 AC-13: every status has the tone and symbol of the table"))
    func everyStatusHasTheToneAndSymbolOfTheTable() {
        let table: [(CheckStatus, Tone, String?)] = [
            (.passed, .positive, "checkmark.circle.fill"),
            (.warning, .caution, "exclamationmark.triangle.fill"),
            (.failed, .critical, "xmark.circle.fill"),
            (.pending, .neutral, nil),
            (.unrecognized, .caution, "questionmark.diamond.fill"),
        ]
        for (status, tone, symbol) in table {
            #expect(status.tone == tone)
            #expect(status.symbol == symbol)
        }
    }

    @Test func everyStatusShapeDiffersSoColorIsNeverTheOnlySign() {
        let symbols = [CheckStatus.passed, .warning, .failed, .unrecognized].compactMap(\.symbol)
        #expect(Set(symbols).count == symbols.count)
        #expect(!symbols.contains { $0.isEmpty })
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

    @Test(
        .evidence(
            covers: ["REQ-024"],
            expected: "Spec 0067 AC-13: the adaptive layout uses two columns in a sheet"))
    func theAdaptiveLayoutUsesTwoColumnsInASheet() {
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

    // MARK: - pending spinner

    @Test func theSpinnerWaitsForLoadingDelayAndDefaultsTo300Milliseconds() {
        #expect(CheckSymbol.spinnerDelay(loadingDelay: nil) == .milliseconds(300))
        #expect(CheckSymbol.spinnerDelay(loadingDelay: .zero) == .zero)
        #expect(CheckSymbol.spinnerDelay(loadingDelay: .seconds(2)) == .seconds(2))
    }

    /// The first frame of a pending row under `loadingDelay`, captured before any task ran.
    @MainActor
    private func firstFrameOfAPendingRow(loadingDelay: Duration?) throws -> SnapshotImage {
        let view = CheckList([CheckItem(id: "r", status: .pending, title: "Checking")])
            .environment(\.loadingDelay, loadingDelay)
            .padding(8)
            .background(Color.white)
        let host = NSHostingView(rootView: view)
        let window = SnapshotWindow(size: CGSize(width: 240, height: 48))
        window.contentView = host
        defer { window.close() }
        window.setContentSize(CGSize(width: 240, height: 48))
        host.frame = CGRect(x: 0, y: 0, width: 240, height: 48)
        host.layoutSubtreeIfNeeded()
        return try SnapshotImage.capture(host)
    }

    @Test(
        .evidence(
            covers: ["REQ-027"],
            expected: "Spec 0067 AC-13: a zero loadingDelay draws the pending spinner at once"
        )
    )
    @MainActor
    func aZeroLoadingDelayShowsThePendingSpinnerInTheFirstFrame() throws {
        let immediate = try firstFrameOfAPendingRow(loadingDelay: .zero)
        let defaulted = try firstFrameOfAPendingRow(loadingDelay: nil)
        let slow = try firstFrameOfAPendingRow(loadingDelay: .seconds(3600))
        #expect(immediate.difference(from: defaulted).pixelCount > 0, "the spinner is drawn")
        #expect(
            defaulted.difference(from: slow).pixelCount == 0,
            "no delay but zero shows the spinner in the first frame")
    }
}
