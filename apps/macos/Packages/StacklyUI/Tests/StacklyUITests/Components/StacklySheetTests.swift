import AppKit
import Foundation
import SwiftUI
import Testing

@testable import StacklyUI

/// The sheet with a footer (spec 0067, AC-12, AC-19): the body height, the reason in the
/// footer, and the source of the sheet, which holds its own host and sets the two
/// shortcuts.
struct StacklySheetTests {
    private static let blocked = MutationAvailability.blocked(
        reason: "stacklyd is unavailable.", since: Date(timeIntervalSince1970: 0))

    private func action(
        mutates: Bool = true, reason: String? = nil, busy: Bool = false
    ) -> StacklyAction {
        StacklyAction(
            id: "create", title: "Create", mutates: mutates, disabledReason: reason,
            isBusy: busy, perform: {})
    }

    // MARK: - body height

    @Test func aShortBodyTakesItsContentHeightAndDoesNotScroll() {
        #expect(SheetMetrics.bodyHeight(content: 220, visibleScreen: 900) == 220)
    }

    @Test func aTallBodyStopsAtTheVisibleScreenMinus160() {
        #expect(SheetMetrics.bodyHeight(content: 3000, visibleScreen: 900) == 740)
        #expect(SheetMetrics.bodyHeight(content: 740, visibleScreen: 900) == 740)
    }

    @Test func aTinyScreenNeverGivesANegativeHeight() {
        #expect(SheetMetrics.bodyHeight(content: 300, visibleScreen: 100) == 0)
    }

    // MARK: - footer reason

    @Test func theFooterWritesTheActionsOwnReason() {
        let availability = action(reason: "Fix the errors above.").availability(gate: .available)
        #expect(SheetFooterReason.text(for: availability) == "Fix the errors above.")
    }

    @Test func theFooterWritesTheGateReasonForAMutatingAction() {
        let availability = action().availability(gate: Self.blocked)
        #expect(SheetFooterReason.text(for: availability) == "stacklyd is unavailable.")
    }

    @Test func theActionsOwnReasonBeatsTheGateReason() {
        let availability = action(reason: "Fix the errors above.").availability(gate: Self.blocked)
        #expect(SheetFooterReason.text(for: availability) == "Fix the errors above.")
    }

    @Test func aReadOnlyActionNeverShowsTheGateReason() {
        let availability = action(mutates: false).availability(gate: Self.blocked)
        #expect(SheetFooterReason.text(for: availability) == nil)
    }

    @Test func anEnabledOrBusyActionHasNoFooterReason() {
        #expect(SheetFooterReason.text(for: action().availability(gate: .available)) == nil)
        #expect(
            SheetFooterReason.text(for: action(busy: true).availability(gate: .available)) == nil)
    }

    // MARK: - layout

    /// The height a view takes at `width`.
    private func height(of view: some View, width: CGFloat) -> CGFloat {
        let host = NSHostingView(rootView: view.frame(width: width))
        host.frame = CGRect(x: 0, y: 0, width: width, height: 10)
        host.layoutSubtreeIfNeeded()
        return host.fittingSize.height
    }

    @Test func theSheetIsAtLeastItsMinimumWidth() {
        let sheet = StacklySheet(title: "New Instance", primary: action(), minWidth: 560) {
            Text("Body")
        }
        let host = NSHostingView(rootView: sheet)
        host.layoutSubtreeIfNeeded()
        #expect(host.fittingSize.width >= 560)
    }

    @Test func aSubtitleAndATileMakeTheHeaderTaller() {
        let plain = StacklySheet(title: "New Instance", primary: action()) { Text("Body") }
        let full = StacklySheet(
            title: "New Instance", subtitle: "PostgreSQL 17.4",
            tile: TileSpec(mark: "Pg", tile: .blue), primary: action()
        ) { Text("Body") }
        #expect(height(of: full, width: 560) > height(of: plain, width: 560))
    }

    // MARK: - source

    private func sheetCode() throws -> String {
        let path =
            "apps/macos/Packages/StacklyUI/Sources/StacklyUI/Components/Sheet/StacklySheet.swift"
        let file = SourceFile(
            path: path, text: try String(contentsOf: SourceTree.url(of: path), encoding: .utf8))
        return file.codeLines.joined(separator: "\n")
    }

    @Test func theSheetHoldsItsOwnActionHostAndKeepsEscapeForCancel() throws {
        let code = try sheetCode()
        #expect(code.contains(".actionHost()"), "a confirmation from inside shows on the sheet")
        #expect(
            code.contains(".interactiveDismissDisabled()"),
            "Escape must run Cancel instead of closing the sheet on its own")
        #expect(code.contains(".presentationBackground(StacklyColor.surfaceWindow)"))
    }

    @Test func onlyTheTwoSheetShortcutsAreSet() throws {
        let code = try sheetCode()
        #expect(code.components(separatedBy: ".keyboardShortcut(").count == 3, "two shortcuts")
        #expect(code.components(separatedBy: ".keyboardShortcut(.cancelAction)").count == 2)
        #expect(code.components(separatedBy: ".keyboardShortcut(.defaultAction)").count == 2)
    }

    @Test func aDestructivePrimaryActionHasNoReturnShortcut() throws {
        let code = try sheetCode()
        let destructive = try #require(code.range(of: "if action.role.isDestructive {"))
        let standard = try #require(
            code.range(of: "} else {", range: destructive.upperBound..<code.endIndex))
        let branch = code[destructive.upperBound..<standard.lowerBound]
        #expect(!branch.contains("keyboardShortcut"), "Return does nothing on a destructive action")
        #expect(branch.contains(".buttonStyle(.bordered)"), "and the button is never prominent")
    }
}
