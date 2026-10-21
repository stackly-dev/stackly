import AppKit
import Foundation
import StacklyClient
import SwiftUI
import Testing

@testable import StacklyUI

/// The mapping of an operation to stages, the progress texts, and the layouts (spec 0067,
/// AC-14, AC-15, AC-19), one test per row of the two tables of the spec.
struct StagedProgressTests {
    private static let stages = [
        StageDefinition(key: "download", title: "Download"),
        StageDefinition(key: "verify", title: "Verify digest"),
        StageDefinition(key: "extract", title: "Extract"),
        StageDefinition(key: "commit", title: "Commit"),
    ]

    private func operation(
        _ state: OperationState, stage: String? = nil,
        progress: OperationProgress = .indeterminate
    ) -> OperationSummary {
        OperationSummary(
            id: UUID(), kind: .installRuntime, resource: nil, state: state, stage: stage,
            progress: progress, waitingOn: nil, cancelRequestedAt: nil, pointOfNoReturnAt: nil,
            failure: nil, recovery: nil, result: nil,
            trigger: OperationTriggerInfo(origin: .user, client: .gui),
            createdAt: Date(timeIntervalSince1970: 0), startedAt: nil,
            updatedAt: Date(timeIntervalSince1970: 0), revision: 1)
    }

    private func value(
        _ state: OperationState, stage: String? = nil,
        progress: OperationProgress = .indeterminate,
        stages: [StageDefinition] = StagedProgressTests.stages
    ) -> StagedProgressValue {
        .from(operation: operation(state, stage: stage, progress: progress), stages: stages)
    }

    private func counted(_ done: UInt64, of total: UInt64) -> OperationProgress {
        .counted(CountedProgress(done: done, total: total))
    }

    private func states(_ value: StagedProgressValue) -> [StageState] {
        value.stages.map(\.state)
    }

    // MARK: - mapping table

    @Test func aQueuedOperationHasEveryStagePendingAndNoBar() {
        let queued = value(.queued)
        #expect(states(queued) == [.pending, .pending, .pending, .pending])
        #expect(queued.runningIndex == nil)
        #expect(queued.fraction == nil && queued.percent == nil)
        #expect(queued.currentTitle == "Queued")
        #expect(queued.outcome == .waiting)
        #expect(!queued.showsBar)
    }

    @Test func aRunningKnownStageMarksTheStagesBeforeItDone() {
        let running = value(.running, stage: "extract", progress: counted(50, of: 100))
        #expect(states(running) == [.done, .done, .running, .pending])
        #expect(running.runningIndex == 2)
        #expect(running.currentTitle == "Extract")
        #expect(running.outcome == .running)
        #expect(running.percent == 62, "(2 × 100 + 50) × 100 / (4 × 100)")
        #expect(running.fraction == 0.625)
        #expect(running.showsBar)
    }

    @Test func aCancelRequestedKnownStageKeepsItsPercentAndSaysCancelling() {
        let cancelling = value(.cancelRequested, stage: "verify", progress: counted(25, of: 100))
        #expect(states(cancelling) == [.done, .running, .pending, .pending])
        #expect(cancelling.runningIndex == 1)
        #expect(cancelling.percent == 31)
        #expect(cancelling.currentTitle == "Cancelling…")
        #expect(cancelling.outcome == .cancelling)
    }

    @Test func aRunningOperationWithoutAStageStartsAtTheFirstStageAtZero() {
        let running = value(.running)
        #expect(states(running) == [.running, .pending, .pending, .pending])
        #expect(running.runningIndex == 0)
        #expect(running.fraction == 0 && running.percent == 0)
        #expect(running.currentTitle == "Download")
        #expect(running.outcome == .running)
        let cancelling = value(.cancelRequested)
        #expect(cancelling.currentTitle == "Cancelling…")
        #expect(cancelling.outcome == .cancelling)
        #expect(cancelling.runningIndex == 0)
    }

    @Test func anUnknownStageOrNoStagesGivesWorkingWithAnIndeterminateBar() {
        let unknown = value(.running, stage: "unpacking")
        #expect(states(unknown) == [.pending, .pending, .pending, .pending])
        #expect(unknown.runningIndex == nil)
        #expect(unknown.fraction == nil && unknown.percent == nil)
        #expect(unknown.currentTitle == "Working…")
        #expect(unknown.outcome == .running)
        #expect(unknown.showsBar)
        let none = value(.running, stage: "download", stages: [])
        #expect(none.stages.isEmpty)
        #expect(none.currentTitle == "Working…")
        let noneNoStage = value(.cancelRequested, stages: [])
        #expect(noneNoStage.currentTitle == "Cancelling…")
        #expect(noneNoStage.outcome == .cancelling)
    }

    @Test func aSucceededOperationHasEveryStageDoneAt100() {
        let done = value(.succeeded, stage: "commit")
        #expect(states(done) == [.done, .done, .done, .done])
        #expect(done.runningIndex == nil)
        #expect(done.fraction == 1 && done.percent == 100)
        #expect(done.currentTitle == "Done")
        #expect(done.outcome == .succeeded)
        #expect(!done.showsBar)
    }

    @Test func aFailedKnownStageMarksItFailedAndNamesIt() {
        let failed = value(.failed, stage: "extract")
        #expect(states(failed) == [.done, .done, .failed, .pending])
        #expect(failed.runningIndex == 2)
        #expect(failed.fraction == nil && failed.percent == nil)
        #expect(failed.currentTitle == "Failed at Extract")
        #expect(failed.outcome == .failed)
    }

    @Test func aFailureWithoutAKnownStageLeavesEveryStagePending() {
        for stage in [nil, "unpacking"] {
            let failed = value(.failed, stage: stage)
            #expect(states(failed) == [.pending, .pending, .pending, .pending])
            #expect(failed.runningIndex == nil)
            #expect(failed.currentTitle == "Failed")
        }
        #expect(value(.failed, stage: "download", stages: []).currentTitle == "Failed")
    }

    @Test func needsRecoveryLooksLikeAFailureButSaysSo() {
        let known = value(.needsRecovery, stage: "commit")
        #expect(states(known) == [.done, .done, .done, .failed])
        #expect(known.runningIndex == 3)
        #expect(known.currentTitle == "Needs Recovery")
        #expect(known.outcome == .needsRecovery)
        let unknown = value(.needsRecovery)
        #expect(states(unknown) == [.pending, .pending, .pending, .pending])
        #expect(unknown.runningIndex == nil)
        #expect(unknown.outcome == .needsRecovery)
    }

    @Test func aCancelledKnownStageMarksItCancelled() {
        let cancelled = value(.cancelled, stage: "verify")
        #expect(states(cancelled) == [.done, .cancelled, .pending, .pending])
        #expect(cancelled.runningIndex == 1)
        #expect(cancelled.fraction == nil && cancelled.percent == nil)
        #expect(cancelled.currentTitle == "Cancelled")
        #expect(cancelled.outcome == .cancelled)
    }

    @Test func aCancelledOperationWithoutAKnownStageLeavesEveryStagePending() {
        for stage in [nil, "unpacking"] {
            let cancelled = value(.cancelled, stage: stage)
            #expect(states(cancelled) == [.pending, .pending, .pending, .pending])
            #expect(cancelled.runningIndex == nil)
            #expect(cancelled.currentTitle == "Cancelled")
        }
    }

    // MARK: - percent

    @Test func theFirstOfOneStageReports29Percent() {
        let single = [StageDefinition(key: "only", title: "Only")]
        let running = value(.running, stage: "only", progress: counted(29, of: 100), stages: single)
        #expect(running.percent == 29)
        #expect(running.fraction == 0.29)
    }

    @Test func indeterminateProgressCountsOnlyTheStagesBefore() {
        let running = value(.running, stage: "extract")
        #expect(running.percent == 50, "2 × 100 / 4")
        #expect(running.fraction == 0.5)
    }

    @Test func aTotalOfZeroFallsBackToTheStageIndex() {
        let running = value(.running, stage: "verify", progress: counted(10, of: 0))
        #expect(running.percent == 25)
    }

    @Test func doneAboveTotalCountsAsTheWholeStage() {
        let running = value(.running, stage: "download", progress: counted(500, of: 100))
        #expect(running.percent == 25, "min(done, total) = 100, so (0 + 100) × 100 / 400")
        #expect(running.fraction == 0.25)
    }

    @Test func thePercentNeverReaches100BeforeTheOperationSucceeded() {
        let last = value(.running, stage: "commit", progress: counted(100, of: 100))
        #expect(last.percent == 99)
        #expect(last.fraction == 1, "the bar uses the same numerator and denominator")
        let single = [StageDefinition(key: "only", title: "Only")]
        #expect(
            value(.running, stage: "only", progress: counted(100, of: 100), stages: single)
                .percent == 99)
    }

    @Test func thePercentIsRoundedDownNotToNearest() {
        let three = (1...3).map { StageDefinition(key: "s\($0)", title: "S\($0)") }
        let running = value(.running, stage: "s2", progress: .indeterminate, stages: three)
        #expect(running.percent == 33)
    }

    @Test func hugeCountsDoNotOverflow() {
        let running = value(
            .running, stage: "extract", progress: counted(UInt64.max / 2, of: UInt64.max))
        #expect(running.percent == 62)
    }

    // MARK: - texts

    @Test func theCompactTextAndSpokenValueOfEveryOutcome() {
        let queued = value(.queued)
        #expect(queued.compactText == "Queued")
        #expect(queued.accessibilityValue == "Queued")

        let running = value(.running, stage: "extract", progress: counted(50, of: 100))
        #expect(running.compactText == "Extract · 62%")
        #expect(running.accessibilityValue == "Stage 3 of 4, Extract, 62 percent")

        let unknown = value(.running, stage: "unpacking")
        #expect(unknown.compactText == "Working…")
        #expect(unknown.accessibilityValue == "Working")

        let cancelling = value(.cancelRequested, stage: "extract", progress: counted(50, of: 100))
        #expect(cancelling.compactText == "Cancelling… · 62%")
        #expect(cancelling.accessibilityValue == "Cancelling, stage 3 of 4, 62 percent")

        let cancellingUnknown = value(.cancelRequested, stage: "unpacking")
        #expect(cancellingUnknown.compactText == "Cancelling…")
        #expect(cancellingUnknown.accessibilityValue == "Cancelling")

        #expect(value(.succeeded).compactText == "Done")
        #expect(value(.succeeded).accessibilityValue == "Done")
        #expect(value(.failed, stage: "extract").compactText == "Failed at Extract")
        #expect(value(.failed, stage: "extract").accessibilityValue == "Failed at Extract")
        #expect(value(.needsRecovery).compactText == "Needs Recovery")
        #expect(value(.needsRecovery).accessibilityValue == "Needs Recovery")
        #expect(value(.cancelled).compactText == "Cancelled")
        #expect(value(.cancelled).accessibilityValue == "Cancelled")
    }

    @Test func aRunningOperationWithoutAStageIsSpokenAsStageOneAtZeroPercent() {
        #expect(value(.running).accessibilityValue == "Stage 1 of 4, Download, 0 percent")
        #expect(value(.running).compactText == "Download · 0%")
    }

    // MARK: - announcements

    @Test func aRunningStageIsAnnouncedByItsTitleWithAPeriod() {
        #expect(value(.running, stage: "verify").stageAnnouncement == "Verify digest.")
    }

    @Test func otherOutcomesAnnounceNoStage() {
        #expect(value(.queued).stageAnnouncement == nil)
        #expect(value(.cancelRequested, stage: "verify").stageAnnouncement == nil)
        #expect(value(.succeeded).stageAnnouncement == nil)
        #expect(value(.failed, stage: "verify").stageAnnouncement == nil)
        #expect(value(.running, stage: "unpacking").stageAnnouncement == nil)
    }

    // MARK: - layout

    /// The size a view takes at `width`.
    private func size(of view: some View, width: CGFloat) -> CGSize {
        let host = NSHostingView(rootView: view.frame(width: width))
        host.frame = CGRect(x: 0, y: 0, width: width, height: 10)
        host.layoutSubtreeIfNeeded()
        return host.fittingSize
    }

    @Test func theStagesWrapToTheNextLineInANarrowWidth() {
        let running = value(.running, stage: "verify", progress: counted(10, of: 100))
        let wide = size(of: StagedProgress(running, layout: .stages), width: 600).height
        let narrow = size(of: StagedProgress(running, layout: .stages), width: 160).height
        #expect(narrow > wide, "stages that do not fit move to the next line")
    }

    @Test func theBarShowsOnlyWhileTheOperationRunsOrWindsDown() {
        let running = value(.running, stage: "verify", progress: counted(10, of: 100))
        let done = value(.succeeded)
        let withBar = size(of: StagedProgress(running, layout: .compact), width: 300).height
        let withoutBar = size(of: StagedProgress(done, layout: .compact), width: 300).height
        #expect(withBar > withoutBar)
    }

    @Test func theStageFlowLayoutPlacesWhatFitsOnOneLine() {
        let flow = StageFlowLayout {
            ForEach(0..<3, id: \.self) { _ in Color.red.frame(width: 50, height: 20) }
        }
        #expect(abs(size(of: flow, width: 400).height - 20) < 0.5)
        #expect(abs(size(of: flow, width: 120).height - 46) < 0.5, "two lines, 6 pt apart")
    }

    // MARK: - source

    @Test func theProgressIsOneSpokenElementWithItsIdentifier() throws {
        let path =
            "apps/macos/Packages/StacklyUI/Sources/StacklyUI/Components/Progress/StagedProgress.swift"
        let file = SourceFile(
            path: path, text: try String(contentsOf: SourceTree.url(of: path), encoding: .utf8))
        let code = file.codeLines.joined(separator: "\n")
        #expect(code.contains(".accessibilityElement(children: .ignore)"))
        #expect(code.contains(".accessibilityValue(value.accessibilityValue)"))
        #expect(code.contains("AccessibilityID.Progress.view"))
        #expect(
            code.contains("StacklyMotion.progress(reduceMotion: reduceMotion)"),
            "the bar value does not animate under Reduce Motion")
    }
}
