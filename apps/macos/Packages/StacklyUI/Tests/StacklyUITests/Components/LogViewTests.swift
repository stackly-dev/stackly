import AppKit
import StacklyTestSupport
import SwiftUI
import Testing

@testable import StacklyUI

/// The log view: the level table, the follow rule, and the layout (spec
/// 0067, AC-16, AC-19).
struct LogViewTests {
    // MARK: - level table

    @Test(
        .evidence(
            covers: ["REQ-026", "NFR-012"],
            expected: "Spec 0067 AC-16: every level has the tone and symbol of the table"))
    func everyLevelHasTheToneAndSymbolOfTheTable() {
        let table: [(LogLevel, Tone?, String?)] = [
            (.normal, nil, nil),
            (.warning, .caution, "exclamationmark.triangle.fill"),
            (.error, .critical, "xmark.circle.fill"),
        ]
        for (level, tone, symbol) in table {
            #expect(level.tone == tone)
            #expect(level.symbol == symbol)
        }
    }

    @Test func aWarningAndAnErrorDifferInSymbolSoColorIsNeverTheOnlySign() {
        #expect(LogLevel.warning.symbol != LogLevel.error.symbol)
    }

    // MARK: - follow rule

    @Test func theBottomToleranceIs24Points() {
        #expect(LogFollow.bottomTolerance == 24)
        #expect(LogFollow.isAtBottom(distance: 0))
        #expect(LogFollow.isAtBottom(distance: 24))
        #expect(!LogFollow.isAtBottom(distance: 24.5))
    }

    @Test(
        .evidence(
            covers: ["REQ-026"],
            expected: "Spec 0067 AC-16: a user scroll that ends above the tolerance stops following"
        ))
    func aUserScrollThatEndsAboveTheToleranceStopsFollowing() {
        for start in [ScrollPhase.interacting, .decelerating] {
            #expect(
                LogFollow.shouldStopFollowing(from: start, to: .idle, distanceFromBottom: 25))
        }
    }

    @Test func aUserScrollThatEndsAtTheBottomKeepsFollowing() {
        for start in [ScrollPhase.interacting, .decelerating] {
            #expect(
                !LogFollow.shouldStopFollowing(from: start, to: .idle, distanceFromBottom: 24))
            #expect(!LogFollow.shouldStopFollowing(from: start, to: .idle, distanceFromBottom: 0))
        }
    }

    @Test func aScrollByTheProgramNeverStopsFollowing() {
        for start in [ScrollPhase.animating, .tracking, .idle] {
            #expect(
                !LogFollow.shouldStopFollowing(from: start, to: .idle, distanceFromBottom: 500))
        }
    }

    @Test func aPhaseThatDoesNotEndInIdleDoesNotEndAUserScroll() {
        #expect(!LogFollow.endsUserScroll(from: .interacting, to: .decelerating))
        #expect(!LogFollow.endsUserScroll(from: .tracking, to: .interacting))
    }

    // MARK: - layout

    /// The height a view takes at `width`.
    private func height(of view: some View, width: CGFloat) -> CGFloat {
        let host = NSHostingView(rootView: view.frame(width: width))
        host.frame = CGRect(x: 0, y: 0, width: width, height: 10)
        host.layoutSubtreeIfNeeded()
        return host.fittingSize.height
    }

    private func log(_ lines: [LogLine]) -> some View {
        LogView(lines, follow: .constant(false)).frame(height: 600)
    }

    @Test func aLongLineWrapsInsteadOfBeingCut() {
        let short = LogLine(id: "1", timeText: "12:00:00.000", source: "stdout", text: "ok")
        let long = LogLine(
            id: "1", timeText: "12:00:00.000", source: "stdout",
            text: String(repeating: "word ", count: 60))
        #expect(
            height(of: LogRow(line: long), width: 420)
                > height(of: LogRow(line: short), width: 420) * 2)
    }

    @Test func aWarningAndAnErrorHaveTheSameRowHeightAsALineWithoutASymbol() {
        let normal = LogLine(id: "1", timeText: "12:00:00.000", source: "stdout", text: "ok")
        let warning = LogLine(
            id: "1", timeText: "12:00:00.000", source: "stdout", text: "ok", level: .warning)
        let error = LogLine(
            id: "1", timeText: "12:00:00.000", source: "stdout", text: "ok", level: .error)
        let base = height(of: LogRow(line: normal), width: 420)
        #expect(abs(height(of: LogRow(line: warning), width: 420) - base) < 3)
        #expect(abs(height(of: LogRow(line: error), width: 420) - base) < 3)
    }

    @Test func aLongerSourcePushesTheTextOfItsOwnLine() {
        let usual = LogLine(id: "1", source: "stderr", text: String(repeating: "word ", count: 12))
        let longer = LogLine(
            id: "1", source: "supervisor/restart-policy",
            text: String(repeating: "word ", count: 12))
        #expect(
            height(of: LogRow(line: longer), width: 300)
                >= height(of: LogRow(line: usual), width: 300))
    }

    @Test func theViewRendersWithAndWithoutLines() {
        #expect(height(of: log([]), width: 420) > 0)
        #expect(height(of: log(ComponentSamples.logStates), width: 420) > 0)
    }
}
