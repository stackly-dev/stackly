import Foundation
import StacklyClient
import StacklyTestSupport
import Testing

@testable import StacklyUI

/// `InstanceLogsModel` (spec 0027, *Layar Logs*; AC-7, AC-11): the reads, the follow, the
/// cursors, the 10 000 row limit, and the states, with a scripted daemon.
@MainActor
@Suite(.timeLimit(.minutes(1)))
struct InstanceLogsModelTests {
    private let instance = UUID(uuidString: "0195f2a0-0000-7000-8000-000000000001") ?? UUID()
    private let sleeper = ManualSleep()

    private func model(_ script: LogsScript, follow: Bool = true) -> InstanceLogsModel {
        let sleeper = sleeper
        let model = InstanceLogsModel(
            instanceId: instance, service: script.service,
            timeZone: TimeZone(identifier: "UTC") ?? .gmt, locale: Locale(identifier: "en_US"),
            sleep: { try await sleeper.sleep(for: $0) })
        if follow == false { model.setFollow(false) }
        return model
    }

    /// Yields until `condition` holds; the suite's time limit ends a wait that never does.
    private func until(_ condition: () -> Bool) async {
        while condition() == false { await Task.yield() }
    }

    private func ids(_ model: InstanceLogsModel) -> [String] {
        model.lines.map(\.id)
    }

    // MARK: - Opening

    @Test(
        .evidence(
            covers: ["REQ-017"],
            expected: "Opening reads newest, then follows after later or head"))
    func openingReadsTheNewestPageThenFollowsAfterItsHead() async {
        let script = LogsScript(LogsScript.page(LogsScript.lines(1...3)))
        let model = model(script)
        model.open()
        await until { script.follows.count == 1 }

        #expect(script.reads.first?.position == .newest)
        #expect(script.reads.first?.limit == 200)
        #expect(script.reads.first?.streams == nil)
        #expect(script.reads.first?.contains == nil)
        #expect(script.follows.first?.after == LogsScript.cursor(3))
        #expect(ids(model) == ["1", "2", "3"])
        #expect(model.phase == .ready)
    }

    @Test func aPageThatStoppedShortFollowsAfterItsLaterCursor() async {
        let page = LogsScript.page(
            [LogsScript.line(7)], later: LogsScript.cursor(9), head: LogsScript.cursor(12))
        let script = LogsScript(page)
        let model = model(script)
        model.open()
        await until { script.follows.count == 1 }

        #expect(script.follows.first?.after == LogsScript.cursor(9))
    }

    @Test(
        .evidence(
            covers: ["REQ-017"],
            expected: "Frames append rows and move resumeAfter"))
    func framesAppendRowsAndMoveTheResumeCursor() async {
        let script = LogsScript(LogsScript.page(LogsScript.lines(1...2)))
        let model = model(script)
        model.open()
        await until { script.follows.count == 1 }

        script.send(LogsScript.frame(LogsScript.lines(3...4)))
        await until { model.rows.count == 4 }

        #expect(model.resumeAfter == LogsScript.cursor(4))
        #expect(model.newestSeq == 4)
    }

    // MARK: - Source and filter

    @Test func changingTheSourceUnfollowsAndReadsTheNewestPageAgain() async {
        let script = LogsScript(LogsScript.page(LogsScript.lines(1...2)))
        let model = model(script)
        model.open()
        await until { script.follows.count == 1 }

        model.setSource(.stderr)
        await until { script.follows.count == 2 }

        #expect(script.unfollows == 1)
        #expect(script.reads.last?.streams == [.stderr])
        #expect(script.follows.last?.streams == [.stderr])
    }

    @Test func theFilterWaitsThreeHundredMillisecondsAndIsSentTrimmed() async {
        let script = LogsScript(LogsScript.page([]))
        let model = model(script, follow: false)
        model.open()
        await until { script.reads.count == 1 }

        model.setQuery("  checkpoint\n ")
        await sleeper.waitUntilSleeping()
        #expect(sleeper.durations == [.milliseconds(300)])
        #expect(script.reads.count == 1)

        sleeper.fire()
        await until { script.reads.count == 2 }
        #expect(script.reads.last?.contains == "checkpoint")
    }

    @Test func aFilterLongerThanTheDaemonTakesIsCut() {
        let model = model(LogsScript())
        model.setQuery(String(repeating: "x", count: 300))
        #expect(model.filterText?.count == 256)
    }

    // MARK: - Follow

    @Test func turningFollowOffKeepsTheRowsAndOnReadsAgain() async {
        let script = LogsScript(LogsScript.page(LogsScript.lines(1...2)))
        let model = model(script)
        model.open()
        await until { script.follows.count == 1 }

        model.setFollow(false)
        await until { script.unfollows == 1 }
        #expect(model.rows.count == 2)

        model.setFollow(true)
        await until { script.follows.count == 2 }
        #expect(script.reads.count == 2)
        #expect(script.reads.last?.position == .newest)
    }

    @Test func aRefusedFollowTurnsFollowOffWithTheReason() async {
        let script = LogsScript(LogsScript.page(LogsScript.lines(1...1)))
        script.answer(
            follow: .rejected(
                ErrorBody(
                    code: "events.too_many_subscriptions", category: .invalidInput,
                    message: "this connection already holds 8 subscriptions", retryable: false,
                    resource: nil, fields: nil, hint: nil, details: nil, operationId: nil,
                    rule: nil, items: nil)))
        let model = model(script)
        model.open()
        await until { model.follow == false }

        #expect(model.followFailure == "this connection already holds 8 subscriptions")
        #expect(model.rows.count == 1)
    }

    @Test func anEndedFollowStopsWithoutGoingOffline() async {
        let script = LogsScript(LogsScript.page(LogsScript.lines(1...1)))
        let model = model(script)
        model.open()
        await until { script.follows.count == 1 }

        script.send(LogsScript.frame([], ended: true))
        script.send(LogsScript.frame(LogsScript.lines(2...2)))
        for _ in 0..<50 { await Task.yield() }

        #expect(model.rows.count == 1)
        #expect(model.isOffline == false)
    }

    // MARK: - Reconnect

    @Test(
        .evidence(
            covers: ["REQ-017"],
            expected: "A closed connection keeps rows; reconnect follows from resumeAfter"))
    func aClosedConnectionKeepsTheRowsAndAReconnectFollowsFromTheCursor() async {
        let script = LogsScript(LogsScript.page(LogsScript.lines(1...2)))
        let model = model(script)
        model.open()
        await until { script.follows.count == 1 }
        script.send(LogsScript.frame(LogsScript.lines(3...3)))
        await until { model.rows.count == 3 }

        script.dropConnection()
        await until { model.isOffline }
        #expect(model.rows.count == 3)

        model.reconnected()
        await until { script.follows.count == 2 }
        #expect(script.follows.last?.after == LogsScript.cursor(3))
        #expect(script.reads.count == 1)

        script.send(LogsScript.frame(LogsScript.lines(9...9), gap: LogGap(reason: .rotated)))
        await until { model.rows.count == 5 }
        #expect(ids(model).suffix(2) == ["gap-9", "9"])
    }

    @Test func aReadWhileOfflineWaitsForTheReconnect() async {
        let script = LogsScript()
        script.answer(
            reads: .failure(.notSent(ClientErrorCatalog.notConnected())),
            .success(LogsScript.page(LogsScript.lines(1...1))))
        let model = model(script)
        model.open()
        await until { model.isOffline }
        #expect(model.phase == .ready)
        #expect(InstanceLogsTab.display(of: model).isPlaceholder("Waiting for stacklyd"))

        model.reconnected()
        await until { model.rows.count == 1 }
    }

    // MARK: - Load Earlier and the limit

    @Test func loadEarlierReadsBeforeTheEarlierCursorAndPrepends() async {
        let script = LogsScript(
            LogsScript.page(LogsScript.lines(11...12), earlier: LogsScript.cursor(11)))
        let model = model(script, follow: false)
        model.open()
        await until { model.rows.count == 2 }

        script.answer(reads: .success(LogsScript.page(LogsScript.lines(9...10))))
        model.loadEarlier()
        await until { model.rows.count == 4 }

        #expect(script.reads.last?.position == .before(LogAnchor(cursor: LogsScript.cursor(11))))
        #expect(ids(model) == ["9", "10", "11", "12"])
        #expect(model.earlier == nil)
    }

    @Test(
        .evidence(
            covers: ["REQ-017"],
            expected: "Load Earlier past 10 000 rows drops the newest and stops Follow"))
    func loadEarlierPastTheLimitDropsTheNewestAndStopsFollowing() async {
        let script = LogsScript(
            LogsScript.page(LogsScript.lines(1_001...10_900), earlier: LogsScript.cursor(1_001)))
        let model = model(script)
        model.open()
        await until { script.follows.count == 1 }

        script.answer(reads: .success(LogsScript.page(LogsScript.lines(801...1_000))))
        model.loadEarlier()
        await until { model.rows.first?.seq == 801 }

        #expect(model.rows.count == 10_000)
        #expect(model.rows.last?.seq == 10_800)
        #expect(model.follow == false)
        #expect(model.resumeAfter == nil)
        await until { script.unfollows == 1 }
    }

    @Test func aFollowPastTheLimitDropsTheOldest() async {
        let script = LogsScript(LogsScript.page(LogsScript.lines(1...9_999)))
        let model = model(script)
        model.open()
        await until { script.follows.count == 1 }

        script.send(LogsScript.frame(LogsScript.lines(10_000...10_002)))
        await until { model.rows.last?.seq == 10_002 }

        #expect(model.rows.count == 10_000)
        #expect(model.rows.first?.seq == 3)
        #expect(model.earlier == LogsScript.cursor(3))
    }

    // MARK: - States

    @Test func anUnreadableStoreShowsTheReasonAndTryAgainReadsAgain() async {
        let script = LogsScript()
        script.answer(
            reads: .failure(.rejected(FixtureLogs.unreadable)),
            .success(LogsScript.page(LogsScript.lines(1...1))))
        let model = model(script)
        model.open()
        await until { model.phase != .loading }
        #expect(model.phase == .unreadable("Permission denied reading captured/stdout."))

        model.retry()
        await until { model.rows.count == 1 }
    }

    @Test func theEmptyStatesFollowTheFilterAndTheScanBudget() async {
        let script = LogsScript(LogsScript.page([]))
        let model = model(script, follow: false)
        model.open()
        await until { model.phase == .ready }
        #expect(InstanceLogsTab.display(of: model).isPlaceholder("No output yet"))

        script.answer(reads: .success(LogsScript.page([], scanLimited: true)))
        model.setSource(.stderr)
        await until { model.scanLimited }
        #expect(
            InstanceLogsTab.display(of: model).isPlaceholder(
                "No matches in the last 8 MB searched."))

        script.answer(reads: .success(LogsScript.page([])))
        model.setSource(.stdout)
        await until { model.scanLimited == false && model.phase == .ready }
        #expect(InstanceLogsTab.display(of: model).isPlaceholder("No lines match this filter."))
    }

    // MARK: - Copy

    @Test(
        .evidence(
            covers: ["REQ-017"],
            expected: "Copy Shown Lines writes date, time, source, and text, without day separators"
        ))
    func copyShownLinesWritesEveryRowWithoutDaySeparators() async {
        let midnight = Date(timeIntervalSince1970: 1_792_368_000)
        let script = LogsScript(
            LogsScript.page([
                LogsScript.line(1, "before", at: midnight.addingTimeInterval(-0.25)),
                LogsScript.line(2, "after", stream: .stderr, at: midnight),
            ]))
        let model = model(script, follow: false)
        model.open()
        await until { model.rows.count == 2 }

        #expect(ids(model) == ["1", "day-2026-10-19", "2"])
        #expect(
            model.shownText
                == "2026-10-18 23:59:59.750 stdout before\n2026-10-19 00:00:00.000 stderr after")
        #expect(model.copyText(of: model.lines[1]) == nil)
        #expect(model.copyText(of: model.lines[2]) == "2026-10-19 00:00:00.000 stderr after")
    }

    @Test func copiedLinesWriteControlsAndBidiOverridesAsEscapesButKeepTabs() async {
        let at = Date(timeIntervalSince1970: 1_792_368_000)
        let script = LogsScript(
            LogsScript.page([
                LogsScript.line(1, "red \u{1B}[31mtext\u{1B}[0m\tcol", at: at),
                LogsScript.line(2, "a\nb\r\u{0}\u{7F}\u{85}\u{9B} x\u{202E}y\u{2066}z", at: at),
                LogsScript.line(3, "数据库 café \u{202F}", at: at),
            ]))
        let model = model(script, follow: false)
        model.open()
        await until { model.rows.count == 3 }

        #expect(
            model.copyText(of: model.lines[0])
                == "2026-10-19 00:00:00.000 stdout red \\u{1b}[31mtext\\u{1b}[0m\tcol")
        #expect(
            model.copyText(of: model.lines[1])
                == "2026-10-19 00:00:00.000 stdout a\\u{a}b\\u{d}\\u{0}\\u{7f}\\u{85}\\u{9b} x\\u{202e}y\\u{2066}z"
        )
        #expect(
            model.copyText(of: model.lines[2]) == "2026-10-19 00:00:00.000 stdout 数据库 café \u{202F}"
        )
        #expect(model.shownText.split(separator: "\n").count == 3)
        #expect(model.shownText.unicodeScalars.contains("\u{1B}") == false)
        // The view still shows the line text as the daemon sent it.
        #expect(model.lines[0].text == "red \u{1B}[31mtext\u{1B}[0m\tcol")
    }
}

extension InstanceLogsTab.Display {
    /// Whether this is a placeholder titled `title`.
    fileprivate func isPlaceholder(_ title: String) -> Bool {
        if case .placeholder(let empty) = self { return empty.title == title }
        return false
    }
}
