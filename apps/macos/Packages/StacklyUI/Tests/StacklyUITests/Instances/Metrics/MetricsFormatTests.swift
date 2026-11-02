import Foundation
import StacklyClient
import Testing

@testable import StacklyUI

/// The words of the Metrics tab (spec 0028, AC-3, AC-4, AC-7, AC-13): the fixed text of every
/// unavailable reason, the labels of scope, the cards, the health line, the graph, and the
/// latency in the Summary rows. A missing number never reads as zero.
struct MetricsFormatTests {
    private typealias UnavailableReason = StacklyClient.UnavailableReason

    private static let utc = TimeZone(identifier: "UTC") ?? .gmt
    private static let locale = Locale(identifier: "en_US")

    private static func usage(
        _ scope: UsageScope = .main, count: UInt64 = 1, measured: UInt64 = 1,
        cpu: NumberMetric = .value(12.34), memory: CountMetric = .value(81_788_928),
        read: CountMetric = .value(1_536), write: CountMetric = .value(0)
    ) -> ResourceUsage {
        ResourceUsage(
            scope: scope, processCount: count, measuredCount: measured, cpu: cpu, memory: memory,
            diskRead: read, diskWrite: write)
    }

    private static func missing(_ reason: UnavailableReason) -> NumberMetric {
        .unavailable(UnavailableContent(reason: reason))
    }

    // MARK: - Unavailable

    @Test(
        .evidence(
            covers: ["REQ-018", "AC-018"],
            expected: "Every reason of an unavailable number has its own fixed text"))
    func everyReasonHasItsFixedText() {
        let expected: [(UnavailableReason, String)] = [
            (.warmingUp, "Measuring…"), (.notPermitted, "macOS denied access"),
            (.processExited, "Process exited"), (.notSupported, "Not supported on this Mac"),
            (.notScanned, "Not scanned yet"), (.unrecognized, "Not available"),
        ]
        for (reason, text) in expected {
            #expect(MetricsFormat.unavailableNote(reason) == text)
            let reading = MetricsFormat.cpu(Self.missing(reason))
            #expect(reading.text == "Unavailable")
            #expect(reading.note == text)
        }
    }

    @Test func aNumberTheBuildCannotReadIsNotAvailableNeverZero() {
        #expect(MetricsFormat.cpu(.unrecognized) == .unavailable(.unrecognized))
        #expect(MetricsFormat.memory(.unrecognized) == .unavailable(.unrecognized))
        #expect(MetricsFormat.rate(.unrecognized).text == "Unavailable")
    }

    @Test func valuesReadWithTheirUnits() {
        #expect(MetricsFormat.cpu(.value(0)) == .value("0.0%"))
        #expect(MetricsFormat.cpu(.value(112.46)) == .value("112.5%"))
        #expect(MetricsFormat.memory(.value(81_788_928)) == .value("81.8 MB"))
        #expect(MetricsFormat.memory(.value(1_234_000_000)) == .value("1.2 GB"))
        #expect(MetricsFormat.rate(.value(1_536)) == .value("1.5 kB/s"))
        #expect(MetricsFormat.rate(.value(0)) == .value("0 bytes/s"))
    }

    // MARK: - Labels

    @Test(
        .evidence(
            covers: ["AC-018"],
            expected:
                "A card names its scope: Main process, Process tree with the count, and how many were measured"
        ))
    func theScopeSubtitleSaysHowManyProcessesWereMeasured() {
        #expect(MetricsFormat.scopeSubtitle(Self.usage()) == "Main process")
        #expect(
            MetricsFormat.scopeSubtitle(Self.usage(.tree, count: 6, measured: 6))
                == "Process tree · 6 processes")
        #expect(
            MetricsFormat.scopeSubtitle(Self.usage(.tree, count: 1, measured: 1))
                == "Process tree · 1 process")
        #expect(
            MetricsFormat.scopeSubtitle(Self.usage(.tree, count: 6, measured: 5))
                == "Process tree · 5 of 6 measured")
    }

    @Test func theGraphAndTheListHaveTheirFixedWords() {
        #expect(MetricsFormat.graphTitle(capacity: 60) == "CPU · last 60 samples")
        #expect(MetricsFormat.sampledEvery(5) == "sampled every 5 s")
        #expect(
            MetricsFormat.moreProcesses(shown: 12, total: 230) == "218 more processes not shown")
        #expect(MetricsFormat.moreProcesses(shown: 12, total: 13) == "1 more process not shown")
        #expect(MetricsFormat.moreProcesses(shown: 12, total: nil) == "More processes not shown")
        #expect(MetricsFormat.moreProcesses(shown: 12, total: 12) == "More processes not shown")
    }

    // MARK: - Cards

    private func cards(
        _ usage: ResourceUsage?, data: DataUsageMetric? = nil, restarts: UInt64 = 2
    ) -> [MetricsFormat.Card] {
        let base = FixtureMetrics.reply(.liveMain, instance: MetricsScript.instance)
        let reply = MetricsReply(
            instanceId: base.instanceId, state: base.state, lifecycle: base.lifecycle,
            intervalSeconds: base.intervalSeconds, capacity: base.capacity,
            historyId: base.historyId, process: base.process, samples: [],
            latestProcesses: nil, processesTruncated: nil, historyReset: false,
            restartTotal: restarts, lastFailure: nil, readiness: base.readiness,
            health: base.health, dataUsage: data ?? base.dataUsage)
        return MetricsFormat.cards(
            usage: usage, reply: reply, timeZone: Self.utc, locale: Self.locale)
    }

    @Test(
        .evidence(
            covers: ["REQ-018", "AC-018"],
            expected:
                "Five cards: CPU, Memory, Disk, Disk I/O, and Restarts with Since created"))
    func fiveCardsShowTheSelectedScope() {
        let cards = cards(Self.usage())

        #expect(cards.map(\.title) == ["CPU", "Memory", "Disk", "Disk I/O", "Restarts"])
        #expect(cards[0].headline == "12.3%")
        #expect(cards[0].subtitle == "Main process")
        #expect(cards[1].headline == "81.8 MB")
        #expect(cards[3].headline == "Read 1.5 kB/s")
        #expect(cards[3].lines == ["Write 0 bytes/s"])
        #expect(cards[4].headline == "2")
        #expect(cards[4].subtitle == "Since created")
        #expect(cards.allSatisfy { $0.isUnavailable == false })
    }

    @Test func beforeTheFirstSampleCardsReadMeasuring() {
        let cards = cards(nil)

        #expect(cards[0].headline == "Unavailable")
        #expect(cards[0].lines == ["Measuring…"])
        #expect(cards[0].isUnavailable)
        #expect(cards[3].lines == ["Measuring…"])
    }

    @Test func aCardWithAMissingNumberShowsUnavailableAndTheReason() {
        let cards = cards(
            Self.usage(
                cpu: Self.missing(.notPermitted),
                memory: .unavailable(.init(reason: .processExited)),
                read: .unavailable(.init(reason: .notSupported)),
                write: .unavailable(.init(reason: .notSupported))))

        #expect(cards[0].headline == "Unavailable")
        #expect(cards[0].lines == ["macOS denied access"])
        #expect(cards[1].lines == ["Process exited"])
        #expect(cards[3].headline == "Unavailable")
        #expect(cards[3].lines == ["Not supported on this Mac"])
    }

    @Test func diskIOWithOneSideMissingSaysWhichSide() {
        let cards = cards(Self.usage(write: .unavailable(.init(reason: .warmingUp))))

        #expect(cards[3].headline == "Read 1.5 kB/s")
        #expect(cards[3].lines == ["Write: Measuring…"])
        #expect(cards[3].isUnavailable == false)
    }

    @Test func diskIOWithTheReadSideMissingLeadsWithTheWriteSide() {
        let cards = cards(Self.usage(read: .unavailable(.init(reason: .notPermitted))))

        #expect(cards[3].headline == "Write 0 bytes/s")
        #expect(cards[3].lines == ["Read: macOS denied access"])
    }

    @Test func diskIOMissingForTwoReasonsNamesEachSide() {
        let cards = cards(
            Self.usage(
                read: .unavailable(.init(reason: .processExited)),
                write: .unavailable(.init(reason: .notSupported))))

        #expect(cards[3].headline == "Unavailable")
        #expect(cards[3].lines == ["Read: Process exited", "Write: Not supported on this Mac"])
        #expect(cards[3].isUnavailable)
    }

    @Test(
        .evidence(
            covers: ["REQ-018"],
            expected: "The Disk card writes the scan time, at least for a partial scan"))
    func theDiskCardReadsTheLastScan() {
        let scan = Date(timeIntervalSince1970: 1_792_300_000 - 240)
        let complete = cards(
            Self.usage(),
            data: .scanned(ScannedUsage(bytes: 1_234_000_000, complete: true, scannedAt: scan)))
        let partial = cards(
            Self.usage(),
            data: .scanned(ScannedUsage(bytes: 4_200_000_000, complete: false, scannedAt: scan)))
        let none = cards(Self.usage(), data: .unavailable(.init(reason: .notScanned)))

        #expect(complete[2].headline == "1.2 GB")
        #expect(complete[2].subtitle?.hasPrefix("Scanned ") == true)
        #expect(partial[2].headline == "at least 4.2 GB")
        #expect(none[2].headline == "Unavailable")
        #expect(none[2].lines == ["Not scanned yet"])
        #expect(none[2].subtitle == nil)
    }

    // MARK: - Health line

    private func reply(
        health: InstanceHealth, readiness: InstanceReadiness
    ) -> MetricsReply {
        let base = FixtureMetrics.reply(.liveMain, instance: MetricsScript.instance)
        return MetricsReply(
            instanceId: base.instanceId, state: base.state, lifecycle: base.lifecycle,
            intervalSeconds: 5, capacity: 60, historyId: base.historyId, process: base.process,
            samples: [], latestProcesses: nil, processesTruncated: nil, historyReset: false,
            restartTotal: 0, lastFailure: nil, readiness: readiness, health: health,
            dataUsage: base.dataUsage)
    }

    private func observed(_ seconds: Int) -> Date {
        Date(timeIntervalSince1970: 1_792_300_000 + Double(seconds))
    }

    @Test(
        .evidence(
            covers: ["AC-018"],
            expected: "The header names the health, why, the time observed, and the probe latency"
        ))
    func theHealthLineNamesStateReasonTimeAndLatency() {
        let health = InstanceHealth(
            state: .healthy, reason: "probe_ok", detail: nil, observedAt: observed(0),
            latencyMs: 3)
        let line = MetricsFormat.healthLine(
            reply(health: health, readiness: .fixture(.ready, reason: "probe_ok")),
            timeZone: Self.utc, locale: Self.locale)

        #expect(line.tone == .positive)
        #expect(line.text.hasPrefix("Healthy · probe ok · "))
        #expect(line.text.hasSuffix(" · 3 ms"))
    }

    @Test func theDetailOfTheDaemonReplacesTheReasonCode() {
        let health = InstanceHealth(
            state: .unhealthy, reason: "probe_failed", detail: "connection refused",
            observedAt: observed(0), latencyMs: nil)
        let line = MetricsFormat.healthLine(
            reply(health: health, readiness: .fixture(.ready, reason: "probe_ok")),
            timeZone: Self.utc, locale: Self.locale)

        #expect(line.tone == .critical)
        #expect(line.text.hasPrefix("Unhealthy · connection refused · "))
        #expect(line.text.contains(" ms") == false)
    }

    @Test func readinessLeadsWhileHealthIsStillUnknown() {
        let readiness = InstanceReadiness(
            state: .notReady, reason: "probe_failed", detail: nil, observedAt: observed(0),
            latencyMs: 12)
        let line = MetricsFormat.healthLine(
            reply(health: .fixture(.unknown, reason: "never_observed"), readiness: readiness),
            timeZone: Self.utc, locale: Self.locale)

        #expect(line.tone == .caution)
        #expect(line.text.hasPrefix("Not ready · probe failed · "))
        #expect(line.text.hasSuffix(" · 12 ms"))
    }

    @Test func aWakeShowsUnknownWithItsReason() {
        let health = InstanceHealth(
            state: .unknown, reason: "waking", detail: "rechecking after sleep",
            observedAt: observed(0), latencyMs: nil)
        let readiness = InstanceReadiness(
            state: .unknown, reason: "waking", detail: nil, observedAt: observed(0),
            latencyMs: nil)
        let line = MetricsFormat.healthLine(
            reply(health: health, readiness: readiness), timeZone: Self.utc, locale: Self.locale)

        #expect(line.tone == .neutral)
        #expect(line.text.hasPrefix("Unknown · rechecking after sleep · "))
    }

    // MARK: - Processes and graph

    @Test func processRowsKeepTheOrderOfTheTreeAndNeverTurnAMissingValueIntoZero() {
        let rows = [
            MetricsScript.row(4242),
            ProcessRow(
                pid: 4243, parentPid: 4242, depth: 2, name: "postgres", isMain: false,
                cpu: Self.missing(.warmingUp), memory: .unavailable(.init(reason: .notPermitted))),
        ]
        let lines: [MetricsFormat.ProcessLine] = MetricsFormat.processLines(rows)

        #expect(lines.map(\.id) == [4242, 4243])
        #expect(lines.map(\.depth) == [0, 2])
        #expect(lines[0].cpu == .value("0.5%"))
        #expect(lines[1].cpu == .unavailable(.warmingUp))
        #expect(lines[1].memory == .unavailable(.notPermitted))
    }

    @Test(
        .evidence(
            covers: ["REQ-018"],
            expected: "A sample whose CPU was not measured leaves a gap in the graph, not a zero"))
    func theGraphSkipsSamplesWithoutACPUValue() {
        let gap = MetricsSample(
            seq: 2, sampledAt: FixtureClock.now,
            main: Self.usage(cpu: Self.missing(.warmingUp)), tree: Self.usage(.tree))
        let samples = [MetricsScript.sample(1, cpu: 4), gap, MetricsScript.sample(3, cpu: 8)]

        #expect(MetricsGraph.points(samples, scope: .main).map(\.seq) == [1, 3])
        #expect(MetricsGraph.points(samples, scope: .tree).map(\.seq) == [1, 2, 3])
        #expect(MetricsGraph.points(samples, scope: .main).map(\.cpu) == [4, 8])
    }

    @Test func theGraphAxisReachesAHundredAndGrowsForABusyTree() {
        func points(_ values: Double...) -> [MetricsGraph.Point] {
            values.enumerated().map { .init(seq: UInt64($0.offset + 1), cpu: $0.element) }
        }
        #expect(MetricsGraph.axisTop([]) == 100)
        #expect(MetricsGraph.axisTop(points(12, 80)) == 100)
        #expect(MetricsGraph.axisTop(points(12, 100)) == 100)
        #expect(MetricsGraph.axisTop(points(12, 112.5)) == 200)
        #expect(MetricsGraph.axisTop(points(250)) == 300)
    }

    // MARK: - Summary rows

    @Test(
        .evidence(
            covers: ["AC-018"],
            expected:
                "The Readiness row of Summary and the inspector appends the probe latency"))
    func theReadinessRowAppendsTheProbeLatency() {
        let timed = InstanceSummary.fixture(
            lifecycle: .running, desired: .running,
            readiness: .fixture(.ready, reason: "probe_ok", latencyMs: 3))
        let untimed = InstanceSummary.fixture(
            lifecycle: .running, desired: .running, readiness: .fixture(.ready, reason: "probe_ok"))
        let failed = InstanceSummary.fixture(
            lifecycle: .running, desired: .running,
            readiness: .fixture(
                .notReady, reason: "probe_failed", detail: "connection refused", latencyMs: 250))

        #expect(ReadinessText.text(for: timed) == "Ready · 3 ms")
        #expect(ReadinessText.text(for: untimed) == "Ready")
        #expect(ReadinessText.text(for: failed) == "Not ready: connection refused · 250 ms")
    }

    @Test func aStoppedInstanceShowsNoLatency() {
        let stopped = InstanceSummary.fixture(
            readiness: .fixture(.ready, reason: "probe_ok", latencyMs: 3))
        #expect(ReadinessText.text(for: stopped) == "—")
    }

    @Test(
        .evidence(
            covers: ["AC-018"],
            expected: "Restarts since the instance was created, or no row without the total"))
    func restartsSinceCreatedReadsTheTotal() {
        #expect(
            DetailFormat.restartsSinceCreated(.fixture(restartTotal: 2)) == "2 since created")
        #expect(DetailFormat.restartsSinceCreated(.fixture()) == "0 since created")
    }
}
