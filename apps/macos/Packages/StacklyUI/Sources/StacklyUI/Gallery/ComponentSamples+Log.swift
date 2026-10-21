#if DEBUG
    import SwiftUI

    /// The sample values of milestone 4 (spec 0067): the log view and the diff view. Taken
    /// from the mockup, with no real credentials.
    extension ComponentSamples {
        // MARK: - Log

        /// Ordinary, warning, and error lines with a time and a source.
        static let logLines: [LogLine] = [
            LogLine(
                id: "1", timeText: "12:04:31.118", source: "stdout",
                text: "database system is ready to accept connections"),
            LogLine(
                id: "2", timeText: "12:04:31.402", source: "stderr",
                text: "checkpoint starting: shutdown immediate", level: .warning),
            LogLine(
                id: "3", timeText: "12:04:32.077", source: "stderr",
                text: "FATAL: could not open file “postgresql.conf”: Permission denied",
                level: .error),
            LogLine(id: "4", timeText: "12:04:32.310", source: "stdout", text: "server stopped"),
        ]

        /// A line whose text is long: it wraps and the columns stay aligned.
        static let logLongLine = LogLine(
            id: "long", timeText: "12:04:33.001", source: "stdout",
            text: "LOG:  starting PostgreSQL 17.4 on aarch64-apple-darwin, compiled by clang "
                + "version 17.0.6, 64-bit, listening on IPv4 address “127.0.0.1”, port 5432, "
                + "and on Unix socket “/Users/dev/Library/Application Support/Stackly/run/.s.PGSQL.5432”"
        )

        /// A line whose source is longer than the template: it shows in full and pushes its
        /// own text.
        static let logLongSource = LogLine(
            id: "source", timeText: "12:04:33.540", source: "supervisor/restart-policy",
            text: "restarting after an unexpected exit")

        /// A line with no time and no source.
        static let logBareLine = LogLine(id: "bare", text: "a line with neither time nor source")

        /// The states of the log gallery section, in one list.
        static let logStates: [LogLine] =
            logLines + [logLongLine, logLongSource, logBareLine]

        /// A line for the live demo: the `n`th one, a warning every fifth.
        static func liveLogLine(_ n: Int) -> LogLine {
            LogLine(
                id: "live-\(n)", timeText: String(format: "12:10:%02d.000", n % 60),
                source: "stdout", text: "heartbeat \(n)", level: n % 5 == 0 ? .warning : .normal)
        }

        // MARK: - Diff

        /// A small config diff: lines added, removed, and left as they were.
        static let diffLines: [DiffLine] = [
            DiffLine(id: 0, kind: .unchanged, text: "listen_addresses = '127.0.0.1'"),
            DiffLine(id: 1, kind: .removed, text: "max_connections = 100"),
            DiffLine(id: 2, kind: .added, text: "max_connections = 200"),
            DiffLine(id: 3, kind: .unchanged, text: "shared_buffers = 128MB"),
            DiffLine(id: 4, kind: .added, text: "log_min_duration_statement = 250"),
        ]

        /// A removed and an added line whose text is long: both wrap.
        static let diffLongLines: [DiffLine] = [
            DiffLine(
                id: 0, kind: .removed,
                text: "search_path = '\"$user\", public, extensions, audit, reporting, archive, "
                    + "staging, scratch'"),
            DiffLine(
                id: 1, kind: .added,
                text: "search_path = '\"$user\", public, extensions, audit, reporting, archive, "
                    + "staging, scratch, tenant_0001, tenant_0002'"),
        ]

        /// What the diff says when nothing differs.
        static let diffEmptyText = "No differences from the saved settings."
    }
#endif
