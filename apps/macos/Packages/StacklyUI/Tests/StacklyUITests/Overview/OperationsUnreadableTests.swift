import Foundation
import StacklyClient
import Testing

@testable import StacklyUI

/// Needs Attention when the operations cannot be read (spec 0022 AC-3, rule O2, 2026-10-22):
/// the state both sections read, the item, its place in the order, and the store that keeps
/// its error while a refresh runs.
@MainActor
struct OperationsUnreadableTests {
    private let locale = Locale(identifier: "en_US")
    private let timeZone = TimeZone(identifier: "UTC") ?? .gmt

    // MARK: - The state

    private func state(
        loaded: Bool = false, failed: Bool = false, unsupported: Bool = false,
        active: [OperationSnapshot] = []
    ) -> AttentionOperations? {
        OverviewPage.operationsState(
            active: active, loaded: loaded, failed: failed, unsupported: unsupported)
    }

    @Test func unsupportedWinsEvenWhenTheStoreStillHoldsTheDataOfAnEarlierDaemon() {
        #expect(state(unsupported: true) == .unsupported)
        #expect(state(loaded: true, unsupported: true) == .unsupported)
        #expect(state(loaded: true, failed: true, unsupported: true) == .unsupported)
    }

    @Test func loadedDataIsKeptWhenALaterRefreshFailed() {
        let active = [OperationSnapshot.fixture(number: 1, kind: .startInstance, state: .running)]
        #expect(state(loaded: true, active: active) == .loaded(active))
        #expect(state(loaded: true, failed: true, active: active) == .loaded(active))
    }

    @Test func anErrorWithoutDataIsFailedAndNothingIsNotYetLoaded() {
        #expect(state(failed: true) == .failed)
        #expect(state() == nil)
    }

    @Test func theOperationsSectionReadsTheSameStateAsNeedsAttention() {
        func content(
            loaded: Bool, failed: Bool, unsupported: Bool
        ) -> String {
            switch OverviewPage.operationsContent(
                running: [], loaded: loaded, failed: failed, unsupported: unsupported, retry: {})
            {
            case .loading: "loading"
            case .failed: "failed"
            case .unsupported: "unsupported"
            case .loaded: "loaded"
            }
        }
        #expect(content(loaded: true, failed: false, unsupported: true) == "unsupported")
        #expect(content(loaded: true, failed: true, unsupported: false) == "loaded")
        #expect(content(loaded: false, failed: true, unsupported: false) == "failed")
        #expect(content(loaded: false, failed: false, unsupported: false) == "loading")
    }

    // MARK: - The gate

    private func items(
        instancesLoaded: Bool = true, operations: AttentionOperations?,
        capacity: CapacityState = CapacityState(attempted: true)
    ) -> [AttentionItem]? {
        OverviewPage.attentionItems(
            summaries: FixtureScenario.instanceCases, instancesLoaded: instancesLoaded,
            operations: operations, capacity: capacity, runtimes: .none, now: FixtureClock.now,
            locale: locale, timeZone: timeZone)
    }

    @Test func onlyAnOperationCenterThatHasNotAnsweredKeepsTheSectionWaiting() {
        #expect(items(operations: nil) == nil)
        #expect(items(instancesLoaded: false, operations: .failed) == nil)
        #expect(items(operations: .failed, capacity: CapacityState()) == nil)
        #expect(items(operations: .failed) != nil)
        #expect(items(operations: .unsupported) != nil)
    }

    // MARK: - The item

    private func rules(
        _ operations: AttentionOperations, capacity: CapacityState = CapacityState(attempted: true),
        instances: [InstanceSummary] = FixtureScenario.instanceCases
    ) -> [AttentionItem] {
        AttentionRules.items(
            instances: instances, operations: operations, capacity: capacity,
            now: FixtureClock.now, locale: locale, timeZone: timeZone)
    }

    @Test func aFailedSnapshotGivesTheErrorItemBesideTheInstanceItems() throws {
        let all = rules(.failed)
        let o2 = try #require(all.first { $0.rule == "O2" })
        #expect(o2.id == "operations")
        #expect(o2.tone == .neutral)
        #expect(o2.title == "Operations unavailable")
        #expect(
            o2.message
                == "Stackly couldn’t read operations. Any that need recovery are missing from this list."
        )
        #expect(o2.destination == .operations)
        #expect(o2.category == .operation)
        #expect(all.filter { $0.rule == "O2" }.count == 1)
        #expect(all.contains { $0.rule == "I1" }, "the instance items stay")
    }

    @Test func anUnsupportedDaemonGivesTheUnsupportedItem() throws {
        let o2 = try #require(rules(.unsupported).first { $0.rule == "O2" })
        #expect(
            o2.message
                == "This version of stacklyd doesn’t list operations. Any that need recovery are missing from this list."
        )
    }

    @Test func loadedOperationsGiveNoO2AndKeepRuleO1() {
        let recovery = OperationSnapshot.fixture(
            number: 4, kind: .createInstance, state: .needsRecovery,
            recovery: RecoveryInfo(
                hint: .resume, cause: .interrupted, error: nil, actions: [.resume, .abandon]))
        let all = rules(.loaded([recovery]))
        #expect(all.contains { $0.rule == "O2" } == false)
        #expect(all.contains { $0.rule == "O1" })
    }

    @Test func o2SitsAfterTheDiskItemsAndBeforeTheNeutralInstanceItem() {
        let adopted = InstanceSummary.fixture(
            number: 1, name: "adopted", lifecycle: .running, desired: .running,
            readiness: .fixture(.ready, reason: "ready"),
            health: InstanceHealth(
                state: .healthy, reason: "ok", detail: nil, observedAt: FixtureClock.now))
        let process = InstanceProcess(
            pid: 4001, startedAt: FixtureClock.minutesAgo(30), adopted: true, adoptable: false)
        let neutralInstance = InstanceSummary(
            id: adopted.id, name: adopted.name, displayName: nil, revision: 1,
            software: adopted.software, provider: adopted.provider, runtimeId: adopted.runtimeId,
            runtimeVersion: adopted.runtimeVersion, lifecycle: adopted.lifecycle,
            desired: adopted.desired, readiness: adopted.readiness, health: adopted.health,
            statusSeq: 1, process: process, lastFailure: nil, restart: adopted.restart,
            blocker: nil, endpoints: adopted.endpoints, leftoverProcesses: nil,
            logsUnavailable: false, ownership: .manual, initialized: true)
        let filling = CapacityState(
            report: CapacityReport(
                checkedAt: FixtureClock.now, totalBytes: 1_000_000_000_000,
                availableBytes: 150_000_000_000, usedPercent: 85, level: .filling),
            attempted: true)
        let rules = rules(.failed, capacity: filling, instances: [neutralInstance]).map(\.rule)
        #expect(rules == ["S3", "O2", "I10"])
    }

    @Test func o2CountsTowardTheSixItemLimit() {
        let failing = (1...7).map {
            InstanceSummary.fixture(
                number: UInt8($0), name: "broken-\($0)", lifecycle: .failed, desired: .running)
        }
        let all = rules(.failed, instances: failing)
        #expect(all.count == 8)
        #expect(all.last?.rule == "O2", "neutral comes after seven critical items")
        #expect(all.count > OverviewCopy.attentionLimit)
    }

    @Test func o2IsReadAsInfoAndOpensOperations() throws {
        let o2 = try #require(rules(.failed).first { $0.rule == "O2" })
        #expect(
            o2.spokenLabel
                == "Info: Operations unavailable. Stackly couldn’t read operations. Any that need recovery are missing from this list."
        )
        #expect(o2.spokenHint == "Opens Operations.")
    }

    // MARK: - The store

    /// An operation center whose snapshot waits for the test, then answers `result`.
    private final class HeldCenter {
        var gate: CheckedContinuation<Void, Never>?
        var holds = false
        var failure: ClientError? = .rejected(PageRig.failure)
        var reads = 0

        func fetch() async throws(ClientError) -> Page<OperationSummary> {
            reads += 1
            if holds { await withCheckedContinuation { gate = $0 } }
            if let failure { throw failure }
            return Page(items: [], next: nil)
        }
    }

    @Test func aRefreshThatRunsKeepsTheErrorAndLoadsNothingUntilItSucceeds() async throws {
        let held = HeldCenter()
        let center = OperationCenter(
            fetchPage: { _ throws(ClientError) in try await held.fetch() },
            fetchOne: { _ throws(ClientError) in nil },
            now: { FixtureClock.now }, sleep: { _ in })
        await center.store.resnapshot()
        #expect(center.store.error != nil)
        #expect(center.store.loadedAt == nil)

        held.holds = true
        let refresh = Task { await center.store.refresh() }
        try await waitUntil { held.gate != nil }
        #expect(center.store.error != nil, "O2 stays while the refresh runs")
        #expect(center.store.loadedAt == nil, "and the section does not go back to progress")
        #expect(
            OverviewPage.operationsState(
                active: center.active, loaded: center.store.loadedAt != nil,
                failed: center.store.error != nil, unsupported: center.store.unsupported)
                == .failed)

        held.failure = nil
        held.holds = false
        held.gate?.resume()
        held.gate = nil
        await refresh.value
        #expect(center.store.error == nil, "a snapshot that succeeds clears the error")
        #expect(center.store.loadedAt != nil)
        #expect(
            OverviewPage.operationsState(
                active: center.active, loaded: center.store.loadedAt != nil,
                failed: center.store.error != nil, unsupported: center.store.unsupported)
                == .loaded([]))
    }

    @Test func o2GoesAwayWhenShellSyncLoadsTheFailedStoreAgainOnCaughtUp() async throws {
        let held = HeldCenter()
        let center = OperationCenter(
            fetchPage: { _ throws(ClientError) in try await held.fetch() },
            fetchOne: { _ throws(ClientError) in nil },
            now: { FixtureClock.now }, sleep: { _ in })
        let sync = ShellSync()
        sync.register(center.store)
        sync.handle(.resnapshot(.noCursor))
        try await waitUntil { sync.isSettled }
        func state() -> AttentionOperations? {
            OverviewPage.operationsState(
                active: center.active, loaded: center.store.loadedAt != nil,
                failed: center.store.error != nil, unsupported: center.store.unsupported)
        }
        #expect(state() == .failed, "the first snapshot failed: item O2")
        let rounds = sync.completedRounds

        // The connection comes back through its cursor, with no new snapshot.
        held.failure = nil
        sync.handle(.caughtUp)
        try await waitUntil { sync.isSettled && center.store.loadedAt != nil }
        #expect(state() == .loaded([]), "the store loaded again and O2 is gone")
        #expect(sync.completedRounds > rounds)

        // A caught up connection with every store loaded reads nothing again.
        let reads = held.reads
        sync.handle(.caughtUp)
        #expect(held.reads == reads)
    }
}
