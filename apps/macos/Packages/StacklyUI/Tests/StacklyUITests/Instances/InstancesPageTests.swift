import Foundation
import StacklyClient
import SwiftUI
import Testing

@testable import StacklyUI

/// The states of the Instances page, its notes, its commands, and the words of its inspector
/// (spec 0022 AC-6 to AC-9, AC-13).
@MainActor
struct InstancesPageTests {
    private let rows = InstanceRowModel.rows(of: FixtureScenario.instanceCases, catalog: nil)

    private func state(
        rows: [InstanceRowModel]?, visible: Int? = nil, filter: InstanceFilter = .all,
        query: String = "", error: ErrorBody? = nil, unsupported: Bool = false,
        waiting: FailureContent? = nil, partial: PartialNote? = nil,
        newInstance: StacklyAction? = nil
    ) -> ViewState<[InstanceRowModel]> {
        InstancesPage.state(
            rows: rows, visibleCount: visible ?? rows?.count ?? 0, filter: filter, query: query,
            error: error, unsupported: unsupported, waiting: waiting,
            loadedAt: rows == nil ? nil : FixtureClock.now, isRefreshing: false, partial: partial,
            browseCatalog: {}, newInstance: newInstance, clearFilters: {})
    }

    private let failure = PageRig.failure

    // MARK: - State

    @Test func theFirstSnapshotIsLoading() {
        guard case .loading = state(rows: nil) else {
            Issue.record("expected loading")
            return
        }
    }

    @Test func aFailureWithoutDataIsAFailedPage() {
        guard case .failed(let content) = state(rows: nil, error: failure) else {
            Issue.record("expected failed")
            return
        }
        #expect(content.title == "Couldn’t Load Instances")
        #expect(content.code == "internal.unexpected")
    }

    @Test func aDaemonThatCannotBeReachedWaitsInsteadOfFailing() {
        let waiting = FailureContent(
            title: "Waiting for stacklyd", message: "m", code: "ipc.endpoint_missing")
        guard case .failed(let content) = state(rows: nil, error: failure, waiting: waiting) else {
            Issue.record("expected failed")
            return
        }
        #expect(content.title == "Waiting for stacklyd")
    }

    @Test func anOlderDaemonIsAnUnsupportedPage() {
        guard case .unsupported(let content) = state(rows: nil, unsupported: true) else {
            Issue.record("expected unsupported")
            return
        }
        #expect(content.title == "Instances Unavailable")
        #expect(
            content.reason == "This version of stacklyd doesn’t list instances. Update Stackly.")
    }

    @Test func aFailedRefreshKeepsTheRowsAndAddsAStaleNote() {
        guard case .loaded(let value, let notes) = state(rows: rows, error: failure) else {
            Issue.record("expected loaded")
            return
        }
        #expect(value.count == rows.count)
        #expect(notes.stale != nil)
    }

    @Test func noInstancesShowBrowseCatalogAndNewInstanceOnlyWhenItExists() {
        guard case .empty(let plain, _) = state(rows: []) else {
            Issue.record("expected empty")
            return
        }
        #expect(plain.title == "No Instances")
        #expect(
            plain.message
                == "Each instance gets its own data directory, port and secret. Start by installing a runtime."
        )
        #expect(plain.primaryAction?.title == "Browse Catalog")
        #expect(plain.primaryAction?.mutates == false)
        #expect(plain.secondaryAction == nil)

        let action = StacklyAction(
            id: "newInstance", title: "New Instance…", mutates: true, perform: {})
        guard case .empty(let withNew, _) = state(rows: [], newInstance: action) else {
            Issue.record("expected empty")
            return
        }
        #expect(withNew.secondaryAction?.title == "New Instance…")
        #expect(withNew.secondaryAction?.mutates == true)
    }

    @Test func aFilterThatLeavesNothingShowsNoResultsWithTheQueryOrTheFilter() {
        guard
            case .empty(let byQuery, _) = state(rows: rows, visible: 0, query: "zzz")
        else {
            Issue.record("expected empty")
            return
        }
        #expect(byQuery.title == "No Results for “zzz”")
        #expect(byQuery.primaryAction?.title == "Clear Filters")
        guard case .empty(let byFilter, _) = state(rows: rows, visible: 0, filter: .failed) else {
            Issue.record("expected empty")
            return
        }
        #expect(byFilter.title == "No Results for “Failed”")
    }

    @Test func aRowStaysAPageEvenWhenItsInstanceFailed() {
        guard case .loaded = state(rows: rows) else {
            Issue.record("expected loaded")
            return
        }
    }

    // MARK: - Partial

    @Test func failedRefetchesBecomeOnePartialNoteWithARetryEach() {
        var retried: [UUID] = []
        let first = FixtureID.uuid(kind: FixtureID.instance, number: 1)
        let unknown = FixtureID.uuid(kind: FixtureID.instance, number: 99)
        let note = InstancesPage.partial(
            failures: [
                first: ItemFailure(error: failure, revision: 2),
                unknown: ItemFailure(error: failure, revision: 3),
            ], names: [first: "pg17-dev"], retry: { retried.append($0) })
        #expect(note?.summary == "Couldn’t refresh 2 instances.")
        #expect(
            note?.items.map(\.name)
                == [unknown.uuidString.lowercased(), "pg17-dev"].sorted {
                    $0.localizedStandardCompare($1) == .orderedAscending
                })
        #expect(note?.items.first?.reason == "The registry could not be read.")
        for item in note?.items ?? [] { item.action?.perform() }
        #expect(Set(retried) == [first, unknown])
        #expect(
            note?.items.allSatisfy { $0.action?.title == "Retry" && $0.action?.mutates == false }
                == true)
    }

    @Test func oneFailedRefetchIsSingular() {
        let id = FixtureID.uuid(kind: FixtureID.instance, number: 1)
        let note = InstancesPage.partial(
            failures: [id: ItemFailure(error: failure, revision: 2)], names: [:], retry: { _ in })
        #expect(note?.summary == "Couldn’t refresh 1 instance.")
        #expect(InstancesPage.partial(failures: [:], names: [:], retry: { _ in }) == nil)
    }

    // MARK: - Commands

    private func published(
        selected: InstanceSummary?, sort: InstanceSortComparator = .default,
        showsInspector: Bool = true
    ) -> (actions: [CommandID: StacklyAction], checked: Set<CommandID>) {
        let rig = PageRig()
        let target = selected.map { summary in
            (
                action: rig.actions.stacklyAction(
                    for: summary, context: InstanceActionContext(report: { _ in })),
                verb: rig.actions.primaryAction(for: summary).commandVerb
            )
        }
        return InstancesCommandActions.publish(
            showsInspector: showsInspector, toggleInspector: {}, sort: sort, setSort: { _ in },
            target: target)
    }

    @Test func thePagePublishesTheInspectorAndSevenSortRows() {
        let (actions, _) = published(selected: nil)
        for command in [
            CommandID.toggleInspector, .sortInstancesByName, .sortInstancesByProvider,
            .sortInstancesByVersion, .sortInstancesByEndpoints, .sortInstancesByStatus,
            .sortInstancesAscending, .sortInstancesDescending,
        ] {
            #expect(actions[command] != nil, "\(command)")
            #expect(actions[command]?.mutates == false, "\(command)")
        }
        #expect(actions[.startInstance] == nil)
        #expect(actions[.stopInstance] == nil)
    }

    @Test func theInspectorCommandChangesItsTitleWithTheState() {
        #expect(
            published(selected: nil, showsInspector: true).actions[.toggleInspector]?.title
                == "Hide Inspector")
        #expect(
            published(selected: nil, showsInspector: false).actions[.toggleInspector]?.title
                == "Show Inspector")
    }

    @Test func theSortRowsShowACheckmarkForTheKeyAndTheDirection() {
        let (_, checked) = published(
            selected: nil, sort: InstanceSortComparator(.version, order: .reverse))
        #expect(checked == [.sortInstancesByVersion, .sortInstancesDescending])
        let (_, defaults) = published(selected: nil)
        #expect(defaults == [.sortInstancesByName, .sortInstancesAscending])
    }

    @Test func aSortRowKeepsTheDirectionAndADirectionRowKeepsTheColumn() {
        var applied: [InstanceSortComparator] = []
        let (actions, _) = InstancesCommandActions.publish(
            showsInspector: true, toggleInspector: {},
            sort: InstanceSortComparator(.provider, order: .reverse),
            setSort: { applied.append($0) },
            target: nil)
        actions[.sortInstancesByStatus]?.perform()
        actions[.sortInstancesAscending]?.perform()
        #expect(
            applied == [
                InstanceSortComparator(.status, order: .reverse),
                InstanceSortComparator(.provider, order: .forward),
            ])
    }

    @Test func aStoppedInstancePublishesStartAndAFailedOneStartAsWell() {
        let stopped = published(selected: .fixture(lifecycle: .stopped)).actions
        #expect(stopped[.startInstance]?.title == "Start")
        #expect(stopped[.startInstance]?.id == "startInstance")
        #expect(stopped[.stopInstance] == nil)
        let failed = published(selected: .fixture(lifecycle: .failed)).actions
        #expect(failed[.startInstance] != nil)
        #expect(failed[.stopInstance] == nil)
    }

    @Test func aRunningInstancePublishesStopAndAnUnknownOneStopWithItsReason() {
        let running = published(selected: .fixture(lifecycle: .running)).actions
        #expect(running[.stopInstance]?.title == "Stop")
        #expect(running[.startInstance] == nil)
        let unknown = published(selected: .fixture(lifecycle: .unknown)).actions
        #expect(unknown[.stopInstance]?.disabledReason == "Stackly can’t confirm this process.")
    }

    @Test func aBusyInstancePublishesTheVerbThatIsGoingOnAsBusy() {
        let starting = published(selected: .fixture(lifecycle: .starting)).actions
        #expect(starting[.startInstance]?.isBusy == true)
        #expect(starting[.stopInstance] == nil)
        let stopping = published(selected: .fixture(lifecycle: .stopping)).actions
        #expect(stopping[.stopInstance]?.isBusy == true)
    }

    @Test func anUninitializedInstancePublishesStartWithItsReason() {
        let start = published(selected: .fixture(lifecycle: .uninitialized)).actions[.startInstance]
        #expect(start?.disabledReason == "This instance isn’t initialized.")
    }

    @Test func openBackAndCopyAreOnlyPublishedWhenTheirTargetExists() {
        let (none, _) = published(selected: nil)
        for command in [CommandID.openInstance, .back, .copyConnectionString] {
            #expect(none[command] == nil, "\(command)")
        }
        var backs = 0
        let open = StacklyAction(id: "open", title: "Open", mutates: false, perform: {})
        let copy = StacklyAction(id: "copy", title: "Copy Connection String", mutates: false) {}
        let (actions, _) = InstancesCommandActions.publish(
            showsInspector: true, toggleInspector: {}, sort: .default, setSort: { _ in },
            target: nil, open: open, back: { backs += 1 }, copyConnection: copy)
        #expect(actions[.openInstance]?.id == "openInstance")
        #expect(actions[.copyConnectionString]?.id == "copyConnectionString")
        #expect(actions[.back]?.title == "Back")
        #expect(actions[.back]?.mutates == false)
        actions[.back]?.perform()
        #expect(backs == 1)
    }

    @Test func findIsNotPublishedWhileADetailPageIsOpen() {
        let (detail, _) = InstancesCommandActions.publish(
            showsInspector: true, toggleInspector: {}, sort: .default, setSort: { _ in },
            target: nil, back: {})
        #expect(detail[.find] == nil)
        #expect(detail[.back] != nil)
        #expect(published(selected: nil).actions[.find] == nil)
    }

    // MARK: - The words of the inspector

    @Test func readinessIsOnlyWrittenForARunningOrRecoveringInstance() throws {
        func text(_ name: String) throws -> String {
            ReadinessText.text(for: try InstanceContractFixture.instance(name))
        }
        #expect(try text("orders-db") == "Ready")
        #expect(try text("analytics-db") == "Waiting for probe")
        #expect(try text("cache") == "Not ready: connection refused")
        #expect(try text("quiet-db") == "Unknown (stale)")
        #expect(try text("flapping") == "Unknown (not_running)")
        #expect(try text("queue") == "—")
        #expect(try text("broken-db") == "—")
    }

    @Test func notReadyWithoutADetailIsJustNotReady() {
        let readiness = InstanceReadiness(
            state: .notReady, reason: "probe_failed", detail: nil, observedAt: FixtureClock.now)
        let base = InstanceSummary.fixture(lifecycle: .running, readiness: readiness)
        #expect(ReadinessText.text(for: base) == "Not ready")
    }

    @Test func theInspectorWritesHealthProcessAndRestartsInWords() {
        #expect(InstanceInspector.healthText(.fixture(.healthy, reason: "ok")) == "Healthy")
        #expect(
            InstanceInspector.healthText(
                .fixture(.degraded, reason: "slow", detail: "slow queries"))
                == "Degraded: slow queries")
        #expect(InstanceInspector.healthText(.fixture(.unhealthy, reason: "x")) == "Unhealthy")
        #expect(InstanceInspector.healthText(.fixture(.unknown, reason: "x")) == "Unknown")

        let now = FixtureClock.now
        let process = InstanceProcess(
            pid: 4001, startedAt: now.addingTimeInterval(-184), adopted: false, adoptable: true)
        #expect(InstanceInspector.processText(process, now: now) == "PID 4001 · up 3m04s")
        let adopted = InstanceProcess(
            pid: 7, startedAt: now.addingTimeInterval(-60), adopted: true, adoptable: nil)
        #expect(InstanceInspector.processText(adopted, now: now) == "PID 7 · up 1m00s · adopted")

        func restart(_ attempts: UInt32, exhausted: Bool = false, window: UInt32 = 120)
            -> InstanceRestart
        {
            InstanceRestart(
                attempts: attempts, budget: 5, windowSeconds: window, nextAttemptAt: nil,
                exhausted: exhausted)
        }
        #expect(InstanceInspector.restartsText(restart(0)) == nil)
        #expect(InstanceInspector.restartsText(restart(2)) == "2 in the last 2 min")
        #expect(InstanceInspector.restartsText(restart(5, exhausted: true)) == "Stopped restarting")
        #expect(InstanceInspector.desiredText(.running) == "Running")
        #expect(InstanceInspector.desiredText(.stopped) == "Stopped")
    }

    @Test func ownershipIsManualAndAnUnknownValueIsUnknown() {
        #expect(InstanceInspector.ownershipText(.manual) == "Manual")
        #expect(InstanceInspector.ownershipText(nil) == "Manual")
        #expect(InstanceInspector.ownershipText(.unrecognized) == "Unknown")
    }

    @Test func theExcerptOfAFailureIsOneLinePerLine() {
        let failure = InstanceLastFailure(
            code: "crash_loop", at: nil, exitCode: nil, signal: nil,
            excerpt: ["[ERR] one\n[ERR] two", "[FTL] three"])
        #expect(InstanceInspector.excerpt(failure) == "[ERR] one\n[ERR] two\n[FTL] three")
        #expect(InstanceInspector.excerpt(nil) == nil)
        #expect(
            InstanceInspector.excerpt(
                InstanceLastFailure(code: "x", at: nil, exitCode: nil, signal: nil, excerpt: []))
                == nil)
    }

    @Test func onlyStartAndRetryAreTheMainActionOfTheInspector() {
        func action(_ lifecycle: InstanceLifecycle) -> InstanceAction {
            InstanceAction.resolve(summary: .fixture(lifecycle: lifecycle), local: .none)
        }
        #expect(InstancesPage.isPrimary(action(.stopped)))
        #expect(InstancesPage.isPrimary(action(.failed)))
        #expect(InstancesPage.isPrimary(action(.running)) == false)
        #expect(InstancesPage.isPrimary(action(.starting)) == false)
    }

    @Test func theBannerOfAnInstanceIsOnlyForTheProblemsAPersonCanFix() throws {
        func banner(_ name: String) throws -> String? {
            AttentionRules.banner(
                for: try InstanceContractFixture.instance(name), now: FixtureClock.now,
                locale: Locale(identifier: "en_US"), timeZone: .gmt)?.rule
        }
        #expect(try banner("broken-db") == "I1")
        #expect(try banner("mystery") == "I2")
        #expect(try banner("locked-db") == "I3")
        #expect(try banner("secretive") == "I4")
        #expect(try banner("sick-db") == nil)
        #expect(try banner("orders-db") == nil)
    }
}
