import Foundation
import StacklyClient
import StacklyTestSupport
import Synchronization
import Testing

@testable import StacklyUI

/// What the model announced, in order.
@MainActor
final class Announcements {
    var items: [String] = []
}

/// `InstanceDetailModel` (spec 0021, *Tab Summary*; AC-9, AC-14, AC-18): what the page holds
/// for one instance, with a scripted daemon and sleeps that never wait.
@MainActor
@Suite(.timeLimit(.minutes(1)))
struct InstanceDetailModelTests {
    private let item: DetailFixture.Triple
    private let script: DetailScript
    private let sleeper = ManualSleep()
    private let announcements = Announcements()

    init() throws {
        item = try DetailFixture.triple("postgresql")
        script = DetailScript(detail: item.detail, usage: item.dataUsage)
    }

    private func model(debounce: Duration = .milliseconds(250)) -> InstanceDetailModel {
        let sleeper = sleeper
        let announcements = announcements
        return InstanceDetailModel(
            instanceId: item.detail.instanceId, service: script.service, debounce: debounce,
            sleep: { try await sleeper.sleep(for: $0) },
            announce: { announcements.items.append($0) })
    }

    // MARK: - Loading

    @Test func openingReadsTheDetailAndTheDataSize() async {
        let model = model()
        #expect(model.isDetailPending)
        #expect(model.dataSizeState == .loaded(nil))

        await model.open()

        #expect(model.detail == item.detail)
        #expect(model.isDetailPending == false)
        #expect(model.usage == item.dataUsage)
        #expect(DetailFormat.dataSize(model.dataSizeState) == "1.3 GB")
        // The page starts a measurement: it is not a cache only read, and not a refresh.
        #expect(script.usageCalls.count == 1)
        #expect(script.usageCalls.first?.refresh == false)
        #expect(script.usageCalls.first?.cachedOnly == false)
    }

    @Test func recalculateAsksTheDaemonToMeasureAgain() async {
        let model = model()
        await model.open()
        await model.loadUsage(refresh: true)

        #expect(script.usageCalls.last?.refresh == true)
        #expect(script.usageCalls.last?.cachedOnly == false)
    }

    @Test func theDataSizeSaysCalculatingWhileTheCallRuns() async {
        let model = model()
        script.answer(usage: .success(nil))
        await model.open()
        #expect(DetailFormat.dataSize(model.dataSizeState) == "—")

        script.answer(usage: .failure(.rejected(.refusal("instance.data_usage_unavailable"))))
        await model.loadUsage(refresh: true)
        #expect(model.dataSizeState == .unavailable)
        #expect(DetailFormat.dataSize(model.dataSizeState) == "Unavailable")
    }

    @Test func aFailedLoadKeepsTheDetailAndAnotherTryRecovers() async {
        let model = model()
        await model.open()

        script.answer(detail: .failure(.notSent(ClientErrorCatalog.notConnected())))
        await model.reload()
        #expect(model.detail == item.detail)
        #expect(model.detailError?.code == "ipc.not_connected")

        script.answer(detail: .success(item.detail))
        await model.reload()
        #expect(model.detailError == nil)
    }

    @Test func aFailedFirstLoadHasNoDetailAndAnError() async {
        script.answer(detail: .failure(.rejected(.refusal("internal.unexpected"))))
        let model = model()
        await model.open()

        #expect(model.detail == nil)
        #expect(model.detailError?.code == "internal.unexpected")
        #expect(model.isDetailPending == false)
        #expect(model.isNotFound == false)
    }

    @Test func anInstanceTheDaemonDoesNotKnowIsNotFound() async {
        script.answer(detail: .failure(.rejected(.refusal("resource.not_found"))))
        let model = model()
        await model.open()

        #expect(model.isNotFound)
        #expect(model.detailError == nil)
    }

    // MARK: - Revisions

    @Test func aReplyOlderThanTheHeldDetailIsDropped() async {
        let model = model()
        await model.open()
        script.answer(detail: .success(item.detail.at(revision: item.detail.revision + 3)))
        await model.reload()
        #expect(model.detail?.revision == item.detail.revision + 3)

        script.answer(detail: .success(item.detail))
        await model.reload()
        #expect(model.detail?.revision == item.detail.revision + 3)
    }

    @Test func aNewerSummaryRevisionReloadsTheDetailAfterThePause() async throws {
        let model = model()
        await model.open()
        model.observe(item.instance)
        #expect(script.detailCalls == 1)

        script.answer(detail: .success(item.detail.at(revision: item.instance.revision + 1)))
        model.observe(item.instance.testing(revision: item.instance.revision + 1))
        await sleeper.waitUntilSleeping()
        #expect(script.detailCalls == 1, "the reload waits for the pause")
        #expect(sleeper.durations.last == .milliseconds(250))

        sleeper.fire()
        try await waitUntil { model.detail?.revision == item.instance.revision + 1 }
        #expect(script.detailCalls == 2)
    }

    @Test func aQuickSecondChangeStartsThePauseOver() async throws {
        let model = model()
        await model.open()
        model.observe(item.instance)

        model.observe(item.instance.testing(revision: item.instance.revision + 1))
        await sleeper.waitUntilSleeping()
        model.observe(item.instance.testing(revision: item.instance.revision + 2))
        await sleeper.waitUntilSleeping()
        script.answer(detail: .success(item.detail.at(revision: item.instance.revision + 2)))
        sleeper.fire()
        try await waitUntil { model.detail?.revision == item.instance.revision + 2 }

        #expect(script.detailCalls == 2, "one reload for two changes")
    }

    @Test func aRevisionTheDetailAlreadyHasReloadsNothing() async {
        let model = model()
        await model.open()
        model.observe(item.instance)
        model.observe(item.instance.testing(statusSeq: 9))

        #expect(sleeper.waiting == 0)
        #expect(script.detailCalls == 1)
    }

    // MARK: - Secrets

    @Test func aSecretChangeForOneOfItsReferencesReloadsTheDetail() async throws {
        let model = model()
        await model.open()
        let watching = Task { await model.watchSecrets() }
        defer { watching.cancel() }
        while script.listenerCount == 0 { await Task.yield() }

        let reference = try #require(item.detail.secrets.first?.reference)
        script.send(.secretChanged("local:other/instance/secret"))
        script.send(.secretChanged(reference, status: .missing, sequence: 2))
        try await waitUntil { script.detailCalls == 2 }
        // Only the second event named a reference of this instance.
        #expect(script.detailCalls == 2)
    }

    // MARK: - Ready announcement

    @Test func readyIsAnnouncedOnceAfterTwoSeconds() async throws {
        let model = model()
        await model.open()
        let waiting = item.instance.testing(
            readiness: InstanceReadiness(
                state: .notReady, reason: "awaiting_probe", detail: nil,
                observedAt: item.instance.readiness.observedAt))
        model.observe(waiting)
        model.observe(item.instance)
        await sleeper.waitUntilSleeping()
        #expect(sleeper.durations.last == .seconds(2))
        #expect(announcements.items.isEmpty)

        sleeper.fire()
        try await waitUntil { announcements.items.count == 1 }
        #expect(announcements.items == ["orders-db is ready."])

        model.observe(item.instance.testing(statusSeq: 5))
        #expect(sleeper.waiting == 0, "the same readiness is not announced twice")
    }

    @Test func readinessThatFlipsBeforeTwoSecondsIsNotAnnounced() async {
        let model = model()
        await model.open()
        let waiting = item.instance.testing(
            readiness: InstanceReadiness(
                state: .notReady, reason: "awaiting_probe", detail: nil,
                observedAt: item.instance.readiness.observedAt))
        model.observe(waiting)
        model.observe(item.instance)
        await sleeper.waitUntilSleeping()
        model.observe(waiting.testing(statusSeq: 7))
        sleeper.fire()
        await Task.yield()

        #expect(announcements.items.isEmpty)
    }

    @Test func anInstanceThatIsReadyOnArrivalIsNotAnnounced() async {
        let model = model()
        await model.open()
        model.observe(item.instance)

        #expect(sleeper.waiting == 0)
        #expect(announcements.items.isEmpty)
    }

    // MARK: - Secret values

    @Test func theModelNeverHoldsASecretValue() async throws {
        let credential = FixtureCredential()
        let model = model()
        await model.open()
        model.observe(item.instance)
        model.close()

        // The only way a value reaches the screen is `secret.reveal` through `RevealModel`;
        // the state of this model is the detail the daemon sent, which carries references.
        credential.assertAbsent(in: String(reflecting: model.detail as Any), what: "the detail")
        var dumped = ""
        dump(model, to: &dumped)
        credential.assertAbsent(in: dumped, what: "a dump of the model")
        #expect(dumped.contains("local:instance/"), "the references are in the detail")
    }
}
