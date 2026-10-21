import AppKit
import SwiftUI
import Testing

@testable import StacklyUI

/// The log view: the spoken line, the level table, the follow rule, and the layout (spec
/// 0067, AC-16, AC-19).
struct LogViewTests {
    // MARK: - level table

    @Test func everyLevelHasTheToneSymbolAndWordOfTheTable() {
        let table: [(LogLevel, Tone?, String?, String?)] = [
            (.normal, nil, nil, nil),
            (.warning, .caution, "exclamationmark.triangle.fill", "Warning"),
            (.error, .critical, "xmark.circle.fill", "Error"),
        ]
        for (level, tone, symbol, word) in table {
            #expect(level.tone == tone)
            #expect(level.symbol == symbol)
            #expect(level.word == word)
        }
    }

    @Test func aWarningAndAnErrorDifferInSymbolSoColorIsNeverTheOnlySign() {
        #expect(LogLevel.warning.symbol != LogLevel.error.symbol)
    }

    // MARK: - spoken line

    @Test func aLineIsReadAsTimeSourceLevelAndText() {
        let line = LogLine(
            id: "1", timeText: "12:04:31.118", source: "stderr", text: "disk full", level: .error)
        #expect(line.accessibilityLabel == "12:04:31.118, stderr, Error: disk full")
        let warning = LogLine(
            id: "2", timeText: "12:04:31.118", source: "stderr", text: "slow", level: .warning)
        #expect(warning.accessibilityLabel == "12:04:31.118, stderr, Warning: slow")
    }

    @Test func aNormalLineLeavesOutTheLevel() {
        let line = LogLine(id: "1", timeText: "12:04:31.118", source: "stdout", text: "ready")
        #expect(line.accessibilityLabel == "12:04:31.118, stdout: ready")
    }

    @Test func aLineLeavesOutAnEmptyTimeAndAnEmptySource() {
        let noTime = LogLine(id: "1", source: "stderr", text: "x", level: .error)
        #expect(noTime.accessibilityLabel == "stderr, Error: x")
        let emptyParts = LogLine(
            id: "2", timeText: "", source: "", text: "x", level: .warning)
        #expect(emptyParts.accessibilityLabel == "Warning: x")
    }

    @Test func aBareNormalLineIsReadAsItsText() {
        #expect(LogLine(id: "1", text: "just text").accessibilityLabel == "just text")
    }

    @Test func theListIdentifierIsFixed() {
        #expect(AccessibilityID.Log.list == "log.list")
    }

    // MARK: - follow rule

    @Test func theBottomToleranceIs24Points() {
        #expect(LogFollow.bottomTolerance == 24)
        #expect(LogFollow.isAtBottom(distance: 0))
        #expect(LogFollow.isAtBottom(distance: 24))
        #expect(!LogFollow.isAtBottom(distance: 24.5))
    }

    @Test func aUserScrollThatEndsAboveTheToleranceStopsFollowing() {
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
