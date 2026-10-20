import Foundation
import StacklyClient
import Testing

@testable import StacklyUI

/// The provisional Instances page: its states, its rows, and the badge count behind it
/// (spec 0013 AC-13, AC-17).
@Suite(.timeLimit(.minutes(1)))
struct ProvisionalInstanceListTests {
    let waiting = FailureContent(
        title: "Waiting for stacklyd",
        message: "Stackly shows this page once the daemon is reachable.",
        code: "ipc.endpoint_missing")

    func rows() throws -> [InstanceRow] {
        [InstanceRow(try ContractFixture.instance())]
    }

    func state(
        rows: [InstanceRow]?, error: ErrorBody? = nil, unsupported: Bool = false,
        waiting: FailureContent? = nil
    ) -> ViewState<[InstanceRow]> {
        ProvisionalInstanceList.state(
            rows: rows, error: error, unsupported: unsupported, waiting: waiting,
            loadedAt: rows == nil ? nil : Date(timeIntervalSince1970: 100), isRefreshing: false)
    }

    // MARK: - States

    @Test func aPageWithoutDataLoads() {
        guard case .loading = state(rows: nil) else {
            Issue.record("expected loading")
            return
        }
    }

    @Test func aPageWithoutDataWaitsForTheDaemonInsteadOfLoading() {
        guard case .failed(let failure) = state(rows: nil, waiting: waiting) else {
            Issue.record("expected failed")
            return
        }
        #expect(failure.title == "Waiting for stacklyd")
        #expect(failure.code == "ipc.endpoint_missing")
    }

    @Test func aStoreErrorWithoutDataIsCouldntLoad() {
        let error = ResourceStoreTests.error("resource.busy")
        guard case .failed(let failure) = state(rows: nil, error: error) else {
            Issue.record("expected failed")
            return
        }
        #expect(failure.title == "Couldn’t Load Instances")
        #expect(failure.code == "resource.busy")
    }

    @Test func waitingBeatsAStoreErrorWhileThereIsNoData() {
        let error = ResourceStoreTests.error("resource.busy")
        guard case .failed(let failure) = state(rows: nil, error: error, waiting: waiting) else {
            Issue.record("expected failed")
            return
        }
        #expect(failure.title == "Waiting for stacklyd")
    }

    @Test func anEmptyListIsNoInstancesWithoutAnyAction() {
        guard case .empty(let empty, let notes) = state(rows: []) else {
            Issue.record("expected empty")
            return
        }
        #expect(empty.title == "No Instances")
        #expect(empty.message == "Create one with stackly instance create.")
        #expect(empty.symbol == SidebarSection.instances.style.symbol)
        #expect(empty.primaryAction == nil)
        #expect(empty.secondaryAction == nil)
        #expect(notes.stale == nil)
    }

    @Test func aPageWithDataIgnoresTheWaitingFailureSoNoStaleNoteAppears() throws {
        guard case .loaded(let value, let notes) = state(rows: try rows(), waiting: waiting) else {
            Issue.record("expected loaded")
            return
        }
        #expect(value.count == 1)
        #expect(notes.stale == nil)
    }

    @Test func aFailedRefreshKeepsTheRowsAndAddsAStaleNote() throws {
        let error = ResourceStoreTests.error("resource.busy")
        guard case .loaded(let value, let notes) = state(rows: try rows(), error: error) else {
            Issue.record("expected loaded")
            return
        }
        #expect(value.count == 1)
        #expect(notes.stale != nil)
    }

    @Test func anOlderDaemonShowsTheUnsupportedPlaceholder() {
        guard case .unsupported(let content) = state(rows: nil, unsupported: true) else {
            Issue.record("expected unsupported")
            return
        }
        #expect(content.title == "Instances Aren’t Supported")
    }

    // MARK: - Rows

    @Test func aRowShowsNameProviderVersionAndStatus() throws {
        let row = try #require(try rows().first)
        #expect(row.name == "orders-db")
        #expect(row.detail == "PostgreSQL 17.2.0")
        #expect(row.appearance.label == "Running")
        #expect(row.spokenLabel == "orders-db, PostgreSQL 17.2.0, Status: Running")
    }

    @Test func aRowNeverShowsStoppedForAnInstanceTheDaemonCalledFailed() throws {
        let cache = try ContractFixture.instance("frame-response-instance-get-failed")
        #expect(InstanceRow(cache).appearance.label == "Failed")
    }

    @Test func aSoftwareThatIsNotListedGetsACapital() {
        #expect(InstanceRow.providerTitle("fake") == "Fake")
        #expect(InstanceRow.providerTitle("nats") == "NATS")
        #expect(InstanceRow.providerTitle("") == "")
    }

    // MARK: - The badge

    @Test func theSidebarBadgeCountsTheLoadedInstancesAndNothingBefore() async throws {
        let fixture = DaemonModelTests.Fixture()
        let orders = try ContractFixture.instance()
        let cache = try ContractFixture.instance("frame-response-instance-get-failed")
        fixture.link.result.withLock {
            $0 = .success(.instanceList(InstancePage(items: [orders, cache], next: nil)))
        }
        #expect(fixture.model.sidebarCounts.instances == nil)
        await fixture.send(.resnapshot(.noCursor))
        try await waitUntil { fixture.model.instances.loadedCount != nil }
        #expect(fixture.model.sidebarCounts.instances == 2)
        #expect(fixture.model.instances.subtitle == "2 instances")
        let badge = try #require(
            SidebarSection.instances.badge(counts: fixture.model.sidebarCounts, isConnected: false))
        #expect(badge.count == 2)
        #expect(badge.spokenValue == "2 instances, last known")
    }

    @Test func aRetainedInstanceIsNotCounted() async throws {
        let fixture = DaemonModelTests.Fixture()
        let orders = try ContractFixture.instance()
        let retained = orders.testing(id: UUID(), name: "old", lifecycle: .retained)
        fixture.link.result.withLock {
            $0 = .success(.instanceList(InstancePage(items: [orders, retained], next: nil)))
        }
        await fixture.send(.resnapshot(.noCursor))
        try await waitUntil { fixture.model.instances.loadedCount != nil }
        #expect(fixture.model.sidebarCounts.instances == 1)
        #expect(fixture.model.instances.subtitle == "1 instance")
    }
}
