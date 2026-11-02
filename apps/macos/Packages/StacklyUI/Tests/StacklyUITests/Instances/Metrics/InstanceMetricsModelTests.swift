import Foundation
import StacklyClient
import StacklyTestSupport
import Testing

@testable import StacklyUI

/// `InstanceMetricsModel` (spec 0028, *Layar Metrics*; AC-11, AC-13): the cursor, the merge of
/// samples and process lists, the polling that runs only while the tab shows, and the states,
/// with a scripted daemon.
@MainActor
@Suite(.timeLimit(.minutes(1)))
struct InstanceMetricsModelTests {
    private let sleeper = ManualSleep()

    private func model(_ script: MetricsScript, scope: InstanceMetricsModel.Scope = .main)
        -> InstanceMetricsModel
    {
        let sleeper = sleeper
        return InstanceMetricsModel(
            instanceId: MetricsScript.instance, service: script.service, scope: scope,
            sleep: { try await sleeper.sleep(for: $0) })
    }

    /// Yields until `condition` holds; the suite's time limit ends a wait that never does.
    private func until(_ condition: () -> Bool) async {
        while condition() == false { await Task.yield() }
    }

    private func seqs(_ model: InstanceMetricsModel) -> [UInt64] {
        model.samples.map(\.seq)
    }

    // MARK: - Polling

    @Test(
        .evidence(
            covers: ["REQ-018", "AC-018"],
            expected:
                "The tab reads at once without a cursor, then every intervalSeconds with the historyId and the newest seq"
        ))
    func openingReadsAtOnceThenEveryIntervalWithTheCursor() async {
        let script = MetricsScript(MetricsScript.reply(samples: MetricsScript.samples(1...3)))
        let model = model(script)
        model.open()
        await sleeper.waitUntilSleeping()

        #expect(
            script.calls == [
                MetricsScript.Call(
                    instance: MetricsScript.instance, historyId: nil, afterSeq: nil)
            ])
        #expect(sleeper.durations == [.seconds(5)])
        #expect(seqs(model) == [1, 2, 3])
        #expect(model.phase == .ready)

        script.answer(
            .success(MetricsScript.reply(samples: MetricsScript.samples(4...5), reset: false)))
        sleeper.fire()
        await until { script.calls.count == 2 }

        #expect(script.calls.last?.historyId == MetricsScript.history)
        #expect(script.calls.last?.afterSeq == 3)
        await until { seqs(model) == [1, 2, 3, 4, 5] }
    }

    @Test func theIntervalFollowsTheReply() async {
        let script = MetricsScript(MetricsScript.reply(samples: [], interval: 2))
        let model = model(script)
        model.open()
        await sleeper.waitUntilSleeping()
        #expect(sleeper.durations == [.seconds(2)])
        #expect(model.intervalSeconds == 2)
    }

    @Test(
        .evidence(
            covers: ["REQ-018"],
            expected: "The tab asks the daemon only while it is visible"))
    func closingTheLastViewStopsThePolling() async {
        let script = MetricsScript(MetricsScript.reply(samples: MetricsScript.samples(1...2)))
        let model = model(script)
        model.open()
        model.open()
        await sleeper.waitUntilSleeping()

        model.close()
        sleeper.fire()
        await until { script.calls.count == 2 }
        await sleeper.waitUntilSleeping()

        model.close()
        sleeper.fire()
        for _ in 0..<50 { await Task.yield() }
        #expect(script.calls.count == 2)
        #expect(sleeper.waiting == 0)
        #expect(seqs(model) == [1, 2])
    }

    // MARK: - Merging

    @Test func aResetReplacesTheWholeHistory() async {
        let script = MetricsScript(MetricsScript.reply(samples: MetricsScript.samples(1...3)))
        let model = model(script)
        model.open()
        await sleeper.waitUntilSleeping()

        script.answer(
            .success(
                MetricsScript.reply(
                    samples: MetricsScript.samples(1...2), reset: true, historyId: "9-1")))
        sleeper.fire()
        await until { seqs(model) == [1, 2] }

        script.answer(.success(MetricsScript.reply(samples: [], reset: false, historyId: "9-1")))
        await sleeper.waitUntilSleeping()
        sleeper.fire()
        await until { script.calls.count == 3 }
        #expect(script.calls.last?.historyId == "9-1")
        #expect(script.calls.last?.afterSeq == 2)
    }

    @Test func aHistoryOfAnotherProcessReplacesTheHistoryEvenWithoutAReset() async {
        let script = MetricsScript(MetricsScript.reply(samples: MetricsScript.samples(1...3)))
        let model = model(script)
        model.open()
        await sleeper.waitUntilSleeping()

        script.answer(
            .success(
                MetricsScript.reply(
                    samples: MetricsScript.samples(1...1), reset: false, historyId: "other")))
        sleeper.fire()
        await until { seqs(model) == [1] }
    }

    @Test func aSampleAlreadyHeldIsNotAddedTwice() async {
        let script = MetricsScript(MetricsScript.reply(samples: MetricsScript.samples(1...3)))
        let model = model(script)
        model.open()
        await sleeper.waitUntilSleeping()

        script.answer(
            .success(MetricsScript.reply(samples: MetricsScript.samples(3...4), reset: false)))
        sleeper.fire()
        await until { seqs(model) == [1, 2, 3, 4] }
    }

    @Test func theHistoryNeverHoldsMoreThanTheCapacity() async {
        let script = MetricsScript(
            MetricsScript.reply(samples: MetricsScript.samples(1...5), capacity: 4))
        let model = model(script)
        model.open()
        await sleeper.waitUntilSleeping()
        #expect(seqs(model) == [2, 3, 4, 5])

        script.answer(
            .success(
                MetricsScript.reply(
                    samples: MetricsScript.samples(6...7), reset: false, capacity: 4)))
        sleeper.fire()
        await until { seqs(model) == [4, 5, 6, 7] }
    }

    @Test(
        .evidence(
            covers: ["REQ-018"],
            expected:
                "The process list of the last reply with a new sample stays while a reply brings none"
        ))
    func theProcessListStaysUntilANewSampleBringsAnother() async {
        let first = [MetricsScript.row(4242), MetricsScript.row(4243, depth: 1)]
        let script = MetricsScript(
            MetricsScript.reply(
                samples: MetricsScript.samples(1...1), processes: first, truncated: true))
        let model = model(script)
        model.open()
        await sleeper.waitUntilSleeping()
        #expect(model.processes == first)
        #expect(model.processesTruncated)

        script.answer(
            .success(MetricsScript.reply(samples: [], reset: false, processes: nil)))
        sleeper.fire()
        await until { script.calls.count == 2 }
        await sleeper.waitUntilSleeping()
        #expect(model.processes == first)
        #expect(model.processesTruncated)

        let next = [MetricsScript.row(4242)]
        script.answer(
            .success(
                MetricsScript.reply(
                    samples: MetricsScript.samples(2...2), reset: false, processes: next)))
        sleeper.fire()
        await until { model.processes == next }
        #expect(model.processesTruncated == false)
    }

    @Test func anInstanceThatStopsDropsTheHistoryAndTheProcesses() async {
        let script = MetricsScript(
            MetricsScript.reply(
                samples: MetricsScript.samples(1...3), processes: [MetricsScript.row(4242)]))
        let model = model(script)
        model.open()
        await sleeper.waitUntilSleeping()
        #expect(model.isLive)

        script.answer(.success(MetricsScript.reply(samples: [], state: .notRunning)))
        sleeper.fire()
        await until { model.isLive == false }
        #expect(model.samples.isEmpty)
        #expect(model.processes.isEmpty)
        #expect(InstanceMetricsTab.display(of: model) == .notRunning)

        // The next live reply starts a history again, without the old cursor.
        script.answer(.success(MetricsScript.reply(samples: MetricsScript.samples(1...1))))
        await sleeper.waitUntilSleeping()
        sleeper.fire()
        await until { script.calls.count == 3 }
        #expect(script.calls.last?.historyId == nil)
        #expect(script.calls.last?.afterSeq == nil)
    }

    // MARK: - States

    @Test(
        .evidence(
            covers: ["REQ-018", "NFR-004"],
            expected: "A refused first read shows a failure with Try Again, and a retry recovers"
        ))
    func aRefusedFirstReadFailsAndARetryRecovers() async {
        let script = MetricsScript()
        script.answer(.failure(.rejected(MetricsScript.refusal)))
        let model = model(script)
        model.open()
        await sleeper.waitUntilSleeping()

        #expect(model.phase == .failed(MetricsScript.refusal))
        #expect(InstanceMetricsTab.display(of: model) == .failed(MetricsScript.refusal))

        script.answer(.success(MetricsScript.reply(samples: MetricsScript.samples(1...1))))
        model.retry()
        await until { model.phase == .ready }
        #expect(InstanceMetricsTab.display(of: model) == .live)
    }

    @Test func aRefusalAfterDataKeepsTheDataAndSaysWhyItDidNotRefresh() async {
        let script = MetricsScript(MetricsScript.reply(samples: MetricsScript.samples(1...2)))
        let model = model(script)
        model.open()
        await sleeper.waitUntilSleeping()

        script.answer(.failure(.rejected(MetricsScript.refusal)))
        sleeper.fire()
        await until { model.refreshFailure != nil }
        #expect(model.phase == .ready)
        #expect(seqs(model) == [1, 2])

        script.answer(.success(MetricsScript.reply(samples: [], reset: false)))
        await sleeper.waitUntilSleeping()
        sleeper.fire()
        await until { model.refreshFailure == nil }
    }

    @Test(
        .evidence(
            covers: ["REQ-018"],
            expected:
                "A closed connection keeps the data and pauses the polling until the shell reconnects"
        ))
    func aClosedConnectionPausesThePollingUntilItReturns() async {
        let script = MetricsScript(MetricsScript.reply(samples: MetricsScript.samples(1...2)))
        let model = model(script)
        model.open()
        await sleeper.waitUntilSleeping()

        script.answer(.failure(.notSent(ClientErrorCatalog.notConnected())))
        sleeper.fire()
        await until { model.isOffline }
        for _ in 0..<50 { await Task.yield() }
        #expect(sleeper.waiting == 0)
        #expect(script.calls.count == 2)
        #expect(seqs(model) == [1, 2])
        #expect(InstanceMetricsTab.display(of: model) == .live)

        script.answer(
            .success(MetricsScript.reply(samples: MetricsScript.samples(3...3), reset: false)))
        model.reconnected()
        await until { seqs(model) == [1, 2, 3] }
        #expect(model.isOffline == false)
        #expect(script.calls.last?.afterSeq == 2)
    }

    @Test func aTabThatNeverConnectedWaitsForTheDaemon() async {
        let script = MetricsScript()
        script.answer(.failure(.notSent(ClientErrorCatalog.notConnected())))
        let model = model(script)
        model.open()
        await until { model.isOffline }

        #expect(InstanceMetricsTab.display(of: model) == .waiting)
    }

    @Test func reconnectingDoesNothingWhenTheTabIsNotOffline() async {
        let script = MetricsScript(MetricsScript.reply(samples: MetricsScript.samples(1...1)))
        let model = model(script)
        model.open()
        await sleeper.waitUntilSleeping()
        model.reconnected()
        for _ in 0..<20 { await Task.yield() }
        #expect(script.calls.count == 1)
    }

    @Test func theSelectedScopeDecidesWhichUsageTheCardsRead() async {
        let script = MetricsScript(MetricsScript.reply(samples: [MetricsScript.sample(1, cpu: 2)]))
        let model = model(script)
        model.open()
        await sleeper.waitUntilSleeping()

        #expect(model.latestUsage?.scope == .main)
        model.scope = .tree
        #expect(model.latestUsage?.scope == .tree)
    }
}
