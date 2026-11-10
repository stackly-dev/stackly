import Foundation
import StacklyClient
import StacklyTestSupport
import Testing

@testable import StacklyUI

/// The list of the Operations page (spec 0035 AC-11 and AC-13): the history pages of
/// `operation.list`, Show More with the cursor, the center's changes by revision, the rule for
/// an operation that is older than the loaded pages, a resnapshot that loads the first page
/// again, and an operation the window opens that is not on a loaded page.
@MainActor
struct OperationHistoryTests {
    private func history(_ fake: OperationsFake, center: OperationCenter? = nil)
        -> (OperationHistory, OperationCenter)
    {
        let center = center ?? OperationsFake.center()
        return (OperationHistory(service: fake.service, center: center), center)
    }

    private func ids(_ history: OperationHistory) -> [UUID] {
        history.items.map(\.id)
    }

    @Test(
        .evidence(
            covers: ["REQ-054"],
            expected:
                "the first page loads newest first and Show More appends the next page through its cursor"
        ))
    func theFirstPageLoadsAndShowMoreAppendsTheNext() async {
        let fake = OperationsFake()
        fake.pages[nil] = OperationPage(
            items: [OperationSample.make(5), OperationSample.make(4)], next: "cursor-4")
        fake.pages["cursor-4"] = OperationPage(
            items: [OperationSample.make(3), OperationSample.make(2)], next: nil)
        let (history, _) = history(fake)

        await history.reload()
        #expect(ids(history) == [OperationSample.id(5), OperationSample.id(4)])
        #expect(history.next == "cursor-4")
        #expect(history.loadedAt != nil)

        await history.showMore()
        #expect(fake.listed == [nil, "cursor-4"])
        #expect(ids(history) == [5, 4, 3, 2].map(OperationSample.id))
        #expect(history.next == nil)
    }

    @Test func aChangeAppliesOnlyWithAHigherRevision() async {
        let fake = OperationsFake()
        fake.pages[nil] = OperationPage(
            items: [OperationSample.make(2, state: .running, revision: 3)], next: nil)
        let (history, _) = history(fake)
        await history.reload()

        history.apply(OperationSample.make(2, state: .queued, revision: 2))
        #expect(history.items.first?.state == .running)
        history.apply(OperationSample.make(2, state: .succeeded, revision: 4))
        #expect(history.items.first?.state == .succeeded)
        #expect(history.items.count == 1)
    }

    @Test func aNewOperationOlderThanTheLoadedPagesWaitsForShowMore() async {
        let fake = OperationsFake()
        fake.pages[nil] = OperationPage(
            items: [OperationSample.make(9), OperationSample.make(8)], next: "cursor-8")
        let (history, _) = history(fake)
        await history.reload()

        // Newer than the oldest row: it joins in its place.
        history.apply(OperationSample.make(10))
        // Older than the oldest row while more pages wait: it waits for Show More.
        history.apply(OperationSample.make(3))
        #expect(ids(history) == [10, 9, 8].map(OperationSample.id))
    }

    @Test func aNewOperationOfAnyAgeJoinsOnceTheWholeHistoryLoaded() async {
        let fake = OperationsFake()
        fake.pages[nil] = OperationPage(
            items: [OperationSample.make(9), OperationSample.make(8)], next: nil)
        let (history, _) = history(fake)
        await history.reload()

        history.apply(OperationSample.make(3))
        #expect(ids(history) == [9, 8, 3].map(OperationSample.id))
    }

    @Test func showMoreKeepsTheHigherRevisionOfAnOperationAlreadyHeld() async {
        let fake = OperationsFake()
        fake.pages[nil] = OperationPage(items: [OperationSample.make(9)], next: "cursor-9")
        fake.pages["cursor-9"] = OperationPage(
            items: [OperationSample.make(8, state: .running, revision: 2)], next: nil)
        let (history, _) = history(fake)
        await history.reload()
        // The window opened 8 before Show More, at a newer revision.
        fake.details[OperationSample.id(8)] = OperationsFake.detail(
            OperationSample.make(8, state: .failed, revision: 5))
        _ = await history.read(OperationSample.id(8))
        #expect(ids(history) == [9, 8].map(OperationSample.id))

        await history.showMore()
        #expect(ids(history) == [9, 8].map(OperationSample.id))
        #expect(history.items.last?.state == .failed)
    }

    @Test(
        .evidence(
            covers: ["REQ-054"],
            expected:
                "an operation the window opens that is older than the loaded pages is read with operation.get and joins in its place; one the daemon no longer knows is marked missing"
        ))
    func anOpenedOperationJoinsInItsPlaceAndAPrunedOneIsMissing() async {
        let fake = OperationsFake()
        fake.pages[nil] = OperationPage(
            items: [OperationSample.make(9), OperationSample.make(8)], next: "cursor-8")
        fake.details[OperationSample.id(2)] = OperationsFake.detail(OperationSample.make(2))
        let (history, _) = history(fake)
        await history.reload()

        let read = await history.read(OperationSample.id(2))
        #expect((try? read.get())?.operation.id == OperationSample.id(2))
        #expect(ids(history) == [9, 8, 2].map(OperationSample.id))

        let gone = await history.read(OperationSample.id(1))
        guard case .failure(.rejected(let body)) = gone else {
            Issue.record("expected resource.not_found")
            return
        }
        #expect(body.code == "resource.not_found")
        #expect(history.missing == [OperationSample.id(1)])

        // The first page loads again: the opened operation stays.
        await history.reload()
        #expect(ids(history) == [9, 8, 2].map(OperationSample.id))
    }

    @Test func aFailedFirstPageKeepsTheRowsAndAFailedShowMoreKeepsTheCursor() async {
        let fake = OperationsFake()
        fake.pages[nil] = OperationPage(items: [OperationSample.make(9)], next: "cursor-9")
        let (history, _) = history(fake)
        await history.reload()

        fake.listError = .rejected(OperationsFake.error("ipc.timeout", "timed out"))
        await history.reload()
        #expect(history.error?.code == "ipc.timeout")
        #expect(ids(history) == [OperationSample.id(9)])

        fake.listError = .notSent(ClientErrorCatalog.notConnected())
        await history.showMore()
        #expect(history.moreError != nil)
        #expect(history.next == "cursor-9")

        await history.reload()
        #expect(history.error == nil)
        #expect(history.moreError == nil)
    }

    @Test(
        .evidence(
            covers: ["REQ-054"],
            expected:
                "the history applies the center's events without a reload and loads its first page again when the center loads a snapshot"
        ))
    func theCentersChangesApplyAndASnapshotLoadsTheFirstPageAgain() async throws {
        let fake = OperationsFake()
        fake.pages[nil] = OperationPage(items: [OperationSample.make(4)], next: nil)
        let center = OperationsFake.center()
        let (history, _) = history(fake, center: center)
        await history.start()
        #expect(fake.listed == [nil])

        // An event of a new operation arrives through the center.
        let started = OperationSample.make(6, state: .running, revision: 1)
        center.store.receive(.operation(started))
        try await waitUntil { history.items.first?.id == started.id }
        // It ends: the center moves it to `recent`, the history keeps it at the end state.
        center.store.receive(.operation(started.with(state: .succeeded, revision: 2)))
        try await waitUntil { history.items.first?.state == .succeeded }

        // A resnapshot of the center loads the first page again.
        fake.pages[nil] = OperationPage(
            items: [OperationSample.make(7), OperationSample.make(4)], next: nil)
        await center.store.refresh()
        try await waitUntil { history.items.first?.id == OperationSample.id(7) }
        #expect(fake.listed == [nil, nil])
        #expect(center.listenerCount == 1)

        history.stop()
        try await waitUntil { center.listenerCount == 0 }
    }

    @Test func theOrderIsCreationTimeThenID() {
        let older = OperationSample.make(1)
        let newer = OperationSample.make(2)
        #expect(OperationHistory.isNewer(newer, than: older))
        #expect(OperationHistory.isNewer(older, than: newer) == false)
        let tie = OperationSample.make(3, createdAt: older.createdAt)
        #expect(OperationHistory.isNewer(tie, than: older))
    }
}
