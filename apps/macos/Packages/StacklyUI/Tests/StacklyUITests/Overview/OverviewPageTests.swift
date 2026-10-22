import Foundation
import StacklyClient
import Testing

@testable import StacklyUI

/// What the Overview shows and when (spec 0022 AC-2, AC-3, AC-5, AC-9): the subtitle, the
/// cards, the gates of the sections, and the words VoiceOver reads.
@MainActor
struct OverviewPageTests {
    private let locale = Locale(identifier: "en_US")

    // MARK: - Copy

    @Test func theSubtitleCountsOnlyWhatIsAboveZero() {
        func counts(
            _ ready: Int, _ stopped: Int, failed: Int = 0, notReady: Int = 0, unknown: Int = 0
        )
            -> InstanceCounts
        {
            var counts = InstanceCounts()
            counts.ready = ready
            counts.stopped = stopped
            counts.failed = failed
            counts.notReady = notReady
            counts.unknown = unknown
            return counts
        }
        #expect(OverviewCopy.subtitle(counts(2, 3)) == "2 running · 3 stopped")
        #expect(
            OverviewCopy.subtitle(counts(2, 3, failed: 1, notReady: 4, unknown: 5))
                == "2 running · 3 stopped · 1 failed · 4 not ready · 5 unknown")
        #expect(OverviewCopy.subtitle(InstanceCounts()) == "0 running · 0 stopped")
    }

    @Test func theLinksAndEmptyLinesUseTheWordsOfTheSpec() {
        #expect(OverviewCopy.showMore(3) == "Show 3 More")
        #expect(OverviewCopy.showLess == "Show Less")
        #expect(OverviewCopy.showAllInstances(9) == "Show All 9 Instances")
        #expect(OverviewCopy.showAllOperations == "Show All in Operations")
        #expect(OverviewCopy.nothingNeedsAttention == "Nothing needs attention.")
        #expect(OverviewCopy.nothingRunning == "Nothing running.")
        #expect(OverviewCopy.attentionLimit == 6)
        #expect(OverviewCopy.instanceLimit == 6)
        #expect(OverviewCopy.operationLimit == 5)
    }

    // MARK: - Cards

    @Test func aCardIsSpokenAsItsLabelItsNumberAndTheNotReadyNote() {
        #expect(
            SummaryCard.spokenLabel(label: "Running", count: 4, note: "2 not ready")
                == "Running, 4, 2 not ready")
        #expect(SummaryCard.spokenLabel(label: "Stopped", count: 0, note: nil) == "Stopped, 0")
        #expect(
            SummaryCard.spokenLabel(label: "Failed", count: nil, note: nil) == "Failed, loading")
    }

    // MARK: - The state of the page

    private func state(
        counts: InstanceCounts?, error: ErrorBody? = nil, unsupported: Bool = false,
        waiting: FailureContent? = nil, partial: PartialNote? = nil
    ) -> ViewState<InstanceCounts> {
        OverviewPage.state(
            counts: counts, error: error, unsupported: unsupported, waiting: waiting,
            loadedAt: counts == nil ? nil : FixtureClock.now, isRefreshing: false, partial: partial)
    }

    @Test func theOverviewIsLoadingFailedOrUnsupportedWithItsOwnTitles() {
        guard case .loading = state(counts: nil) else {
            Issue.record("expected loading")
            return
        }
        guard case .failed(let failure) = state(counts: nil, error: PageRig.failure) else {
            Issue.record("expected failed")
            return
        }
        #expect(failure.title == "Couldn’t Load Overview")
        guard case .unsupported(let content) = state(counts: nil, unsupported: true) else {
            Issue.record("expected unsupported")
            return
        }
        #expect(content.title == "Overview Unavailable")
        #expect(
            content.reason == "This version of stacklyd doesn’t list instances. Update Stackly.")
    }

    @Test func theOverviewIsNeverEmptyEvenWithoutInstances() {
        guard case .loaded(_, let notes) = state(counts: InstanceCounts()) else {
            Issue.record("expected loaded")
            return
        }
        #expect(notes.stale == nil)
    }

    @Test func aFailedRefreshKeepsTheCountsAndAddsAStaleNote() {
        guard case .loaded(_, let notes) = state(counts: InstanceCounts(), error: PageRig.failure)
        else {
            Issue.record("expected loaded")
            return
        }
        #expect(notes.stale != nil)
    }

    // MARK: - Needs Attention waits for its sources

    private func items(
        instancesLoaded: Bool = true, operationsLoaded: Bool = true,
        capacity: CapacityState = CapacityState(attempted: true)
    ) -> [AttentionItem]? {
        OverviewPage.attentionItems(
            summaries: FixtureScenario.instanceCases, instancesLoaded: instancesLoaded,
            operations: [], operationsLoaded: operationsLoaded, capacity: capacity,
            runtimes: .none, now: FixtureClock.now, locale: locale, timeZone: .gmt)
    }

    @Test func theItemsWaitForTheInstancesTheOperationsAndTheFirstDiskReading() {
        #expect(items(instancesLoaded: false) == nil)
        #expect(items(operationsLoaded: false) == nil)
        #expect(items(capacity: CapacityState()) == nil)
        #expect(items()?.isEmpty == false)
    }

    @Test func theItemsComeFromTheSharedRulesOnceEverySourceLoaded() {
        let ids = items()?.map(\.id) ?? []
        #expect(Set(ids).count == ids.count)
        #expect(items()?.contains { $0.rule == "I1" } == true)
    }

    // MARK: - Operations section

    private func running(_ count: Int, state: OperationState = .running) -> [OperationSnapshot] {
        (1...max(count, 1)).prefix(count).map {
            .fixture(number: UInt8($0), kind: .startInstance, state: state)
        }
    }

    @Test func theSectionShowsAtMostFiveRunningOperationsAndHowManyThereAre() {
        guard
            case .loaded(let rows, let total) = OverviewPage.operationsContent(
                running: running(7), loaded: true, failed: false, unsupported: false, retry: {})
        else {
            Issue.record("expected loaded")
            return
        }
        #expect(rows.count == 5)
        #expect(total == 7)
    }

    @Test func anOperationThatNeedsRecoveryIsNotInTheOperationsSection() {
        var operations = running(2)
        operations.append(.fixture(number: 9, kind: .createInstance, state: .needsRecovery))
        guard
            case .loaded(let rows, let total) = OverviewPage.operationsContent(
                running: operations, loaded: true, failed: false, unsupported: false, retry: {})
        else {
            Issue.record("expected loaded")
            return
        }
        #expect(rows.count == 2)
        #expect(total == 2)
    }

    @Test func theSectionLoadsFailsOrIsUnsupportedOnItsOwn() {
        func content(loaded: Bool, failed: Bool, unsupported: Bool = false) -> String {
            switch OverviewPage.operationsContent(
                running: [], loaded: loaded, failed: failed, unsupported: unsupported, retry: {})
            {
            case .loading: "loading"
            case .failed: "failed"
            case .unsupported: "unsupported"
            case .loaded: "loaded"
            }
        }
        #expect(content(loaded: false, failed: false) == "loading")
        #expect(content(loaded: false, failed: true) == "failed")
        #expect(content(loaded: true, failed: true) == "loaded")
        #expect(content(loaded: false, failed: false, unsupported: true) == "unsupported")
    }

    // MARK: - Operation rows

    @Test func anInstallHasStagesAndAStartDoesNot() {
        #expect(OperationStages.definitions(for: .installRuntime)?.count == 5)
        #expect(OperationStages.definitions(for: .createInstance)?.count == 6)
        #expect(OperationStages.definitions(for: .startInstance) == nil)
        #expect(OperationStages.definitions(for: .unrecognized) == nil)
    }

    @Test func aStageWithoutADefinitionIsItsKeyWithSpacesAndACapital() {
        func label(_ stage: String?, state: OperationState = .running) -> String {
            OperationStages.label(
                for: .fixture(number: 1, kind: .startInstance, state: state, stage: stage))
        }
        #expect(label("verify_digest") == "Verify digest")
        #expect(label("downloading") == "Downloading")
        #expect(label(nil) == "Running")
        #expect(label(nil, state: .queued) == "Queued")
        #expect(label("") == "Running")
    }

    @Test func anOperationRowIsSpokenWithItsTitleStateStageAndPercent() {
        let install = OperationSnapshot.fixture(
            number: 1, kind: .installRuntime, state: .running, stage: "download",
            progress: .counted(CountedProgress(done: 42, total: 100)))
        #expect(
            OverviewOperationRow.spokenLabel(operation: install, title: "Install PostgreSQL 17.2")
                == "Install PostgreSQL 17.2, Running, Download, 8 percent")
        let start = OperationSnapshot.fixture(number: 2, kind: .startInstance, state: .queued)
        #expect(
            OverviewOperationRow.spokenLabel(operation: start, title: "Start orders-db")
                == "Start orders-db, Queued, Queued")
    }

    // MARK: - Words of the cards of the page, from the rig

    @Test func theCardsOfTheMixedFixtureCountEveryGroup() throws {
        let counts = InstanceCounts(of: FixtureScenario.instanceCases)
        #expect(counts.total == 11)
        #expect(counts.ready == 2)
        #expect(counts.notReady == 3)
        #expect(counts.stopped == 3)
        #expect(counts.failed == 2)
        #expect(counts.unknown == 1)
    }

    @Test func theRigLoadsTheCapacityAndTheOperationsOfTheFixture() async {
        let rig = PageRig(capacity: InstanceContractFixture.capacity(.low))
        await rig.load()
        #expect(rig.capacity.report?.level == .low)
        #expect(rig.capacity.attempted)
        #expect(rig.operations.runningCount == 1)
    }
}
