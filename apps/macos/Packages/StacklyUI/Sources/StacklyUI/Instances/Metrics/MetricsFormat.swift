import Foundation
import StacklyClient

/// The words of the Metrics tab (spec 0028, AC-4, AC-13). Pure functions of the DTOs, so the
/// tab, the snapshots, and the tests say the same thing, and the CLI says it in the same terms.
nonisolated enum MetricsFormat {
    /// The reason of the wire, not the status reason of the same name in this module.
    typealias UnavailableReason = StacklyClient.UnavailableReason

    // MARK: - Readings

    /// One number as the tab shows it: the text of a measured value, or the reason it is
    /// missing. There is no reading of zero for a value that was not measured.
    enum Reading: Equatable, Sendable {
        /// A measured value, already written.
        case value(String)
        /// No value, and why.
        case unavailable(UnavailableReason)

        /// What stands in the place of the number: the value, or "Unavailable".
        var text: String {
            switch self {
            case .value(let text): text
            case .unavailable: "Unavailable"
            }
        }

        /// The fixed line that says why a number is missing, or `nil` for a value.
        var note: String? {
            switch self {
            case .value: nil
            case .unavailable(let reason): MetricsFormat.unavailableNote(reason)
            }
        }
    }

    /// The fixed text for a reason (spec 0028, AC-13).
    /// - Parameter reason: Why there is no value.
    static func unavailableNote(_ reason: UnavailableReason) -> String {
        switch reason {
        case .warmingUp: "Measuring…"
        case .notPermitted: "macOS denied access"
        case .processExited: "Process exited"
        case .notSupported: "Not supported on this Mac"
        case .notScanned: "Not scanned yet"
        case .unrecognized: "Not available"
        }
    }

    /// CPU as a percent of one core with one decimal: "12.5%".
    /// - Parameter metric: The CPU metric.
    static func cpu(_ metric: NumberMetric) -> Reading {
        switch metric {
        case .value(let percent): .value(String(format: "%.1f%%", percent))
        case .unavailable(let content): .unavailable(content.reason)
        case .unrecognized: .unavailable(.unrecognized)
        }
    }

    /// A size in bytes, written as the Details card writes one: "78.0 MB", "1.2 GB".
    /// - Parameter metric: The memory metric.
    static func memory(_ metric: CountMetric) -> Reading {
        switch metric {
        case .value(let bytes): .value(DetailFormat.bytes(bytes))
        case .unavailable(let content): .unavailable(content.reason)
        case .unrecognized: .unavailable(.unrecognized)
        }
    }

    /// Bytes per second: "1.5 kB/s".
    /// - Parameter metric: The disk read or write metric.
    static func rate(_ metric: CountMetric) -> Reading {
        switch memory(metric) {
        case .value(let text): .value(text + "/s")
        case .unavailable(let reason): .unavailable(reason)
        }
    }

    // MARK: - Labels

    /// The subtitle of a card (spec 0028, AC-4): "Main process", "Process tree · 6 processes",
    /// or "Process tree · 5 of 6 measured".
    /// - Parameter usage: The usage the card shows.
    static func scopeSubtitle(_ usage: ResourceUsage) -> String {
        switch usage.scope {
        case .main, .unrecognized:
            return "Main process"
        case .tree:
            if usage.measuredCount < usage.processCount {
                return "Process tree · \(usage.measuredCount) of \(usage.processCount) measured"
            }
            let noun = usage.processCount == 1 ? "process" : "processes"
            return "Process tree · \(usage.processCount) \(noun)"
        }
    }

    /// The title of the graph: "CPU · last 60 samples".
    /// - Parameter capacity: The most samples a history keeps.
    static func graphTitle(capacity: UInt64) -> String {
        "CPU · last \(capacity) samples"
    }

    /// "sampled every 5 s".
    /// - Parameter seconds: The seconds between two samples.
    static func sampledEvery(_ seconds: UInt32) -> String {
        "sampled every \(seconds) s"
    }

    /// The line under the process list when the tree had more processes than the list holds:
    /// "7 more processes not shown", or the same without a number when it is not known.
    /// - Parameters:
    ///   - shown: The rows listed.
    ///   - total: The processes of the tree in the newest sample, or `nil`.
    static func moreProcesses(shown: Int, total: UInt64?) -> String {
        guard let total, total > UInt64(shown) else { return "More processes not shown" }
        let rest = total - UInt64(shown)
        return "\(rest) more \(rest == 1 ? "process" : "processes") not shown"
    }

    // MARK: - Cards

    /// One card of the row of five.
    struct Card: Equatable, Identifiable, Sendable {
        /// The card's identity.
        let id: String
        /// "CPU", "Memory", "Disk", "Disk I/O", or "Restarts".
        let title: String
        /// The large line: the value, or "Unavailable".
        let headline: String
        /// Lines under the headline: a second value, or the reason a value is missing.
        let lines: [String]
        /// The small line at the bottom: the scope, "Scanned 3:04 PM", or "Since created".
        let subtitle: String?
        /// Whether the headline stands for a missing value.
        let isUnavailable: Bool
    }

    /// The five cards for the selected scope (spec 0028, AC-13).
    /// - Parameters:
    ///   - usage: The usage of the selected scope in the newest sample, or `nil` before the
    ///     first sample, which reads as warming up.
    ///   - reply: The newest reply, for the data size and the restarts.
    ///   - timeZone: The zone the scan time is written in.
    ///   - locale: The locale of the scan time.
    static func cards(
        usage: ResourceUsage?, reply: MetricsReply, timeZone: TimeZone, locale: Locale
    ) -> [Card] {
        let warming = Reading.unavailable(.warmingUp)
        let subtitle = usage.map(scopeSubtitle)
        return [
            card("cpu", "CPU", usage.map { cpu($0.cpu) } ?? warming, subtitle),
            card("memory", "Memory", usage.map { memory($0.memory) } ?? warming, subtitle),
            disk(reply.dataUsage, timeZone: timeZone, locale: locale),
            diskIO(usage, subtitle: subtitle),
            Card(
                id: "restarts", title: "Restarts", headline: "\(reply.restartTotal)", lines: [],
                subtitle: "Since created", isUnavailable: false),
        ]
    }

    private static func card(
        _ id: String, _ title: String, _ reading: Reading, _ subtitle: String?
    ) -> Card {
        Card(
            id: id, title: title, headline: reading.text,
            lines: reading.note.map { [$0] } ?? [], subtitle: subtitle,
            isUnavailable: reading.note != nil)
    }

    /// The Disk card: the size of the data folder from the last scan (spec 0028, AC-10).
    private static func disk(_ usage: DataUsageMetric, timeZone: TimeZone, locale: Locale) -> Card {
        switch usage {
        case .scanned(let scan):
            let size = DetailFormat.bytes(scan.bytes)
            let time = scan.scannedAt.formatted(
                Date.FormatStyle(
                    date: .omitted, time: .shortened, locale: locale, timeZone: timeZone))
            return Card(
                id: "disk", title: "Disk", headline: scan.complete ? size : "at least " + size,
                lines: [], subtitle: "Scanned \(time)", isUnavailable: false)
        case .unavailable(let content):
            return card("disk", "Disk", .unavailable(content.reason), nil)
        case .unrecognized:
            return card("disk", "Disk", .unavailable(.unrecognized), nil)
        }
    }

    /// The Disk I/O card: read and write per second. Both measured, it shows "Read" on the
    /// headline and "Write" under it. One side missing, the side that was measured leads and
    /// the other says why it is missing. Both missing, it reads as one missing value with its
    /// reason, or one reason per side when they differ.
    private static func diskIO(_ usage: ResourceUsage?, subtitle: String?) -> Card {
        guard let usage else {
            return card("diskio", "Disk I/O", .unavailable(.warmingUp), nil)
        }
        let read = rate(usage.diskRead)
        let write = rate(usage.diskWrite)
        switch (read, write) {
        case (.value(let reading), .value(let writing)):
            return diskIOCard("Read \(reading)", ["Write \(writing)"], subtitle, false)
        case (.value(let reading), .unavailable):
            return diskIOCard("Read \(reading)", ["Write: \(note(of: write))"], subtitle, false)
        case (.unavailable, .value(let writing)):
            return diskIOCard("Write \(writing)", ["Read: \(note(of: read))"], subtitle, false)
        case (.unavailable(let first), .unavailable(let second)):
            if first == second { return card("diskio", "Disk I/O", read, subtitle) }
            return diskIOCard(
                "Unavailable", ["Read: \(note(of: read))", "Write: \(note(of: write))"], subtitle,
                true)
        }
    }

    private static func diskIOCard(
        _ headline: String, _ lines: [String], _ subtitle: String?, _ isUnavailable: Bool
    ) -> Card {
        Card(
            id: "diskio", title: "Disk I/O", headline: headline, lines: lines, subtitle: subtitle,
            isUnavailable: isUnavailable)
    }

    private static func note(of reading: Reading) -> String {
        reading.note ?? reading.text
    }

    // MARK: - Header

    /// The line at the right of the header: the health of the instance, why, when it was
    /// observed, and how long the probe took (spec 0028, AC-7).
    struct HealthLine: Equatable, Sendable {
        /// The tone of the dot.
        let tone: Tone
        /// "Healthy · probe ok · 9:41:02 AM · 3 ms".
        let text: String
    }

    /// The health line of a reply. Health leads; while it is unknown and readiness is not,
    /// readiness stands in, because a provider without a health plan has only readiness.
    /// - Parameters:
    ///   - reply: The reply.
    ///   - timeZone: The zone the time is written in.
    ///   - locale: The locale of the time.
    static func healthLine(_ reply: MetricsReply, timeZone: TimeZone, locale: Locale) -> HealthLine
    {
        let health = reply.health
        let readiness = reply.readiness
        let useReadiness = health.state == .unknown && readiness.state != .unknown
        let word: String
        let tone: Tone
        let why: String
        let at: Date
        let latency: UInt32?
        if useReadiness {
            switch readiness.state {
            case .ready: (word, tone) = ("Ready", .positive)
            case .notReady: (word, tone) = ("Not ready", .caution)
            case .unknown: (word, tone) = ("Unknown", .neutral)
            }
            why = readiness.detail ?? readable(readiness.reason)
            at = readiness.observedAt
            latency = readiness.latencyMs
        } else {
            switch health.state {
            case .healthy: (word, tone) = ("Healthy", .positive)
            case .degraded: (word, tone) = ("Degraded", .caution)
            case .unhealthy: (word, tone) = ("Unhealthy", .critical)
            case .unknown: (word, tone) = ("Unknown", .neutral)
            }
            why = health.detail ?? readable(health.reason)
            at = health.observedAt
            latency = health.latencyMs
        }
        let time = at.formatted(
            Date.FormatStyle(date: .omitted, time: .standard, locale: locale, timeZone: timeZone))
        var parts = [word]
        if why.isEmpty == false { parts.append(why) }
        parts.append(time)
        if let latency { parts.append("\(latency) ms") }
        return HealthLine(tone: tone, text: parts.joined(separator: " · "))
    }

    /// A reason code as words: "probe_ok" is "probe ok".
    private static func readable(_ reason: String) -> String {
        reason.replacingOccurrences(of: "_", with: " ")
    }

    // MARK: - Processes

    /// One row of the process list, written.
    struct ProcessLine: Equatable, Identifiable, Sendable {
        /// The process ID, which identifies the row.
        let id: UInt32
        /// The depth in the tree; the name is indented by it.
        let depth: Int
        /// The executable's file name.
        let name: String
        /// True for the main process.
        let isMain: Bool
        /// The memory, or why it is missing.
        let memory: Reading
        /// The CPU, or why it is missing.
        let cpu: Reading
    }

    /// The rows of the process list in the order the daemon sent them (the tree order).
    /// - Parameter rows: The process rows.
    static func processLines(_ rows: [ProcessRow]) -> [ProcessLine] {
        rows.map {
            ProcessLine(
                id: $0.pid, depth: Int(clamping: $0.depth), name: $0.name, isMain: $0.isMain,
                memory: memory($0.memory), cpu: cpu($0.cpu))
        }
    }
}
