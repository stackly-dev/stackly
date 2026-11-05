import Foundation
import StacklyClient

/// One loaded row of the Logs tab: an entry or a gap, mapped once when it arrives, with what
/// Copy Shown Lines writes for it and the local day it belongs to (spec 0027, *Layar Logs*).
nonisolated struct ShownLog: Equatable, Sendable {
    /// The row as the log view draws it.
    let line: LogLine
    /// The entry's `seq`, or `nil` for a gap.
    let seq: UInt64?
    /// The local day, `yyyy-MM-dd`, or `nil` for a gap.
    let day: String?
    /// The long local date of ``day``, the text of its separator.
    let dayTitle: String?
    /// What Copy Shown Lines and Copy Line write.
    let copyText: String
}

/// Maps log entries to rows in one time zone (spec 0027, *Pemetaan ke `LogLine`*), with the
/// same texts as `stackly instance logs`. The time zone is the one the mapper was made with,
/// so a change of the system zone applies to the rows mapped after it.
struct LogLineMapper {
    /// The source of markers and gaps.
    static let stacklySource = "stackly"

    let timeZone: TimeZone
    let locale: Locale
    private let time: DateFormatter
    private let stamp: DateFormatter
    private let dayKey: DateFormatter
    private let dayTitle: Date.FormatStyle

    /// Creates a mapper.
    /// - Parameters:
    ///   - timeZone: The zone of the times and days.
    ///   - locale: The locale of the long date of a day separator.
    init(timeZone: TimeZone, locale: Locale = .current) {
        self.timeZone = timeZone
        self.locale = locale
        time = Self.formatter("HH:mm:ss.SSS", timeZone)
        stamp = Self.formatter("yyyy-MM-dd HH:mm:ss.SSS", timeZone)
        dayKey = Self.formatter("yyyy-MM-dd", timeZone)
        var style = Date.FormatStyle(date: .complete, time: .omitted, locale: locale)
        style.timeZone = timeZone
        dayTitle = style
    }

    private static func formatter(_ format: String, _ timeZone: TimeZone) -> DateFormatter {
        let formatter = DateFormatter()
        formatter.locale = Locale(identifier: "en_US_POSIX")
        formatter.calendar = Calendar(identifier: .gregorian)
        formatter.timeZone = timeZone
        formatter.dateFormat = format
        return formatter
    }

    /// The row of an entry.
    func shown(_ entry: LogEntry) -> ShownLog {
        let (text, level) = Self.text(of: entry.kind)
        let source = Self.source(of: entry)
        return ShownLog(
            line: LogLine(
                id: String(entry.seq), timeText: time.string(from: entry.at), source: source,
                text: text, level: level),
            seq: entry.seq, day: dayKey.string(from: entry.at),
            dayTitle: entry.at.formatted(dayTitle),
            copyText: "\(stamp.string(from: entry.at)) \(source) \(Self.pasteSafe(text))")
    }

    /// The row of a gap before the entry `next`, or before the head when no entry follows.
    func shown(_ gap: LogGap, before next: UInt64) -> ShownLog {
        let text =
            switch gap.reason {
            case .rotated: "older lines were rotated out"
            case .storeReset: "the log store was reset"
            case .unrecognized: "some lines could not be read"
            }
        return ShownLog(
            line: LogLine(
                id: "gap-\(next)", source: Self.stacklySource, text: text, level: .warning),
            seq: nil, day: nil, dayTitle: nil, copyText: "\(Self.stacklySource) \(text)")
    }

    /// The rows of an entry list with its gap first.
    func shown(entries: [LogEntry], gap: LogGap?, head: UInt64?) -> [ShownLog] {
        var rows: [ShownLog] = []
        if let gap {
            rows.append(shown(gap, before: entries.first?.seq ?? head ?? 0))
        }
        rows.append(contentsOf: entries.map(shown))
        return rows
    }

    /// `text` with every C0 control but tab, DEL, C1 control, and bidi override or isolate
    /// (U+202A to U+202E, U+2066 to U+2069) written as `\u{..}` in lowercase hex, the format
    /// of the CLI's `escape_display`, so copied lines never carry a terminal escape or a
    /// newline that fakes another line into a paste.
    static func pasteSafe(_ text: String) -> String {
        guard text.unicodeScalars.contains(where: needsEscape) else { return text }
        var out = ""
        out.reserveCapacity(text.utf8.count + 8)
        for scalar in text.unicodeScalars {
            if needsEscape(scalar) {
                out += "\\u{\(String(scalar.value, radix: 16))}"
            } else {
                out.unicodeScalars.append(scalar)
            }
        }
        return out
    }

    /// Whether `scalar` must not reach the pasteboard raw.
    private static func needsEscape(_ scalar: Unicode.Scalar) -> Bool {
        switch scalar.value {
        case 0x09: false
        case 0x00...0x1F, 0x7F...0x9F, 0x202A...0x202E, 0x2066...0x2069: true
        default: false
        }
    }

    /// The source column: the stream of a line, `stackly` for a marker.
    static func source(of entry: LogEntry) -> String {
        guard case .line = entry.kind else { return stacklySource }
        switch entry.stream {
        case .stdout: return "stdout"
        case .stderr: return "stderr"
        case .unrecognized, nil: return stacklySource
        }
    }

    /// The text and level of an entry, the texts of `stackly instance logs`.
    static func text(of kind: LogEntryKind) -> (String, StacklyUI.LogLevel) {
        switch kind {
        case .line(let line):
            let level: StacklyUI.LogLevel =
                switch line.level {
                case .error: .error
                case .warning: .warning
                case .normal, .unrecognized: .normal
                }
            return (line.text, level)
        case .started(let started):
            return ("process started (pid \(started.pid))", .normal)
        case .exited(let exited):
            switch (exited.code, exited.signal) {
            case (0?, _): return ("process exited with code 0", .normal)
            case (let code?, _): return ("process exited with code \(code)", .error)
            case (nil, let signal?): return ("process exited with signal \(signal)", .error)
            case (nil, nil): return ("process exited", .normal)
            }
        case .truncated(let truncated):
            return ("\(byteText(truncated.bytes)) of output was dropped", .warning)
        case .unavailable:
            return ("output while Stackly was not running could not be read", .warning)
        case .unrecognized:
            return ("an entry this version cannot show", .normal)
        }
    }

    /// Bytes for people, like the CLI: `512 bytes`, `1.5 KiB`, `1.0 MiB`, `2.0 GiB`.
    static func byteText(_ bytes: UInt64) -> String {
        let kib = 1024.0
        let value = Double(bytes)
        if value < kib { return "\(bytes) bytes" }
        let (scaled, unit) =
            value < kib * kib
            ? (value / kib, "KiB")
            : value < kib * kib * kib
                ? (value / (kib * kib), "MiB") : (value / (kib * kib * kib), "GiB")
        return String(format: "%.1f %@", scaled, unit)
    }

    /// The rows as the log view draws them: a day separator between two entries whose local
    /// day differs, with no time and no source.
    static func lines(_ rows: [ShownLog]) -> [LogLine] {
        var lines: [LogLine] = []
        lines.reserveCapacity(rows.count + 1)
        var lastDay: String?
        for row in rows {
            if let day = row.day {
                if let lastDay, lastDay != day, let title = row.dayTitle {
                    lines.append(LogLine(id: "day-\(day)", text: title))
                }
                lastDay = day
            }
            lines.append(row.line)
        }
        return lines
    }
}
