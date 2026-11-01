#if DEBUG
    import Foundation
    import StacklyClient

    /// The Logs tab fixtures (spec 0027, *Layar Logs*; spec 0068): the views of the tab in
    /// each of its states, answered from data with no daemon.
    nonisolated enum FixtureLogs {
        /// A state of the tab, named like the snapshots.
        enum Case: String, CaseIterable, Sendable {
            /// PostgreSQL lines of both streams, a warning, and an error, while following.
            case follow = "logs-follow"
            /// A store that holds nothing yet.
            case empty = "logs-empty"
            /// A filter that matches no line.
            case filterEmpty = "logs-filter-empty"
            /// Every marker: started, exited, truncated, unavailable, a gap, and a day change.
            case markers = "logs-markers"
            /// A store the daemon cannot read.
            case unreadable = "logs-unreadable"
        }

        /// The store of every fixture.
        static let storeId = FixtureID.uuid(kind: FixtureID.instance, number: 200)
        /// What the stores keep: 5 files × 20 MiB per stream.
        static let retention = LogRetention(segmentsPerStream: 5, segmentMaxBytes: 20 << 20)

        /// The refusal of a store the daemon cannot read.
        static let unreadable = ErrorBody(
            code: "instance.logs_unreadable", category: .unavailable,
            message: "the log store of the instance can't be read", retryable: false,
            resource: nil, fields: nil, hint: nil,
            details: [
                ErrorDetail(key: "reason", value: "Permission denied reading captured/stdout.")
            ],
            operationId: nil, rule: nil, items: nil)

        /// The model of a case before it opened; the tab gives it the zone and locale of its
        /// environment.
        @MainActor
        static func model(_ fixture: Case, instance: UUID) -> InstanceLogsModel {
            InstanceLogsModel(
                instanceId: instance, service: service(fixture),
                source: fixture == .filterEmpty ? .stderr : .all,
                query: fixture == .filterEmpty ? "checkpoint" : "")
        }

        /// The service of a case: the read answers its page; a follow stays open and sends
        /// nothing, so the tab shows Follow on.
        static func service(_ fixture: Case) -> InstanceLogsService {
            InstanceLogsService(
                read: { _ throws(ClientError) in
                    if fixture == .unreadable {
                        throw .rejected(unreadable)
                    }
                    return page(fixture)
                },
                follow: { _ throws(ClientError) in
                    let (frames, _) = DaemonConnection.EventStream.makeStream()
                    let head = page(fixture).head
                    return LogFollowSession(
                        following: LogsFollowing(
                            subscriptionId: 1, storeId: storeId, head: head),
                        frames: frames, unfollow: {})
                })
        }

        /// The newest page of a case.
        static func page(_ fixture: Case) -> LogPage {
            let entries: [LogEntry] =
                switch fixture {
                case .follow: followEntries
                case .markers: markerEntries
                case .empty, .filterEmpty, .unreadable: []
                }
            let head = (entries.last?.seq).map { LogCursor(storeId: storeId, seq: $0) }
            return LogPage(
                storeId: fixture == .empty ? nil : storeId, entries: entries,
                earlier: fixture == .follow ? LogCursor(storeId: storeId, seq: 1_000) : nil,
                later: nil,
                head: fixture == .filterEmpty ? LogCursor(storeId: storeId, seq: 2_400) : head,
                gap: fixture == .markers ? LogGap(reason: .rotated) : nil,
                scanLimited: false, retention: retention,
                storedBytes: LogStreamBytes(stdout: 4_096, stderr: 1_024), skippedRecords: 0)
        }

        private static func line(
            _ seq: UInt64, _ at: Date, _ stream: LogStream, _ text: String,
            _ level: StacklyClient.LogLevel = .normal
        ) -> LogEntry {
            LogEntry(
                seq: seq, at: at, stream: stream,
                kind: .line(
                    LogLineEntry(
                        text: text, level: level, partial: nil, continued: nil, step: nil)))
        }

        private static func at(_ milliseconds: Int) -> Date {
            FixtureClock.secondsAgo(60).addingTimeInterval(Double(milliseconds) / 1_000)
        }

        private static let followEntries: [LogEntry] = [
            line(1_001, at(0), .stderr, "LOG:  starting PostgreSQL 17.6 on aarch64-apple-darwin"),
            line(1_002, at(4), .stderr, "LOG:  listening on IPv4 address \"127.0.0.1\", port 5432"),
            line(1_003, at(9), .stderr, "LOG:  database system is ready to accept connections"),
            line(1_004, at(2_310), .stdout, "server started"),
            line(
                1_005, at(15_120), .stderr,
                "WARNING:  password=<secret:password> was redacted from this line", .warning),
            line(1_006, at(31_002), .stderr, "LOG:  checkpoint starting: time"),
            line(
                1_007, at(42_517), .stderr,
                "ERROR:  relation \"orders\" does not exist at character 15", .error),
            line(1_008, at(42_518), .stderr, "STATEMENT:  SELECT * FROM orders;"),
        ]

        /// Entries on both sides of the UTC midnight before ``FixtureClock/now``.
        private static var markerEntries: [LogEntry] {
            var calendar = Calendar(identifier: .gregorian)
            calendar.timeZone = TimeZone(identifier: "UTC") ?? .gmt
            let midnight = calendar.startOfDay(for: FixtureClock.now)
            func near(_ seconds: Double) -> Date { midnight.addingTimeInterval(seconds) }
            return [
                LogEntry(
                    seq: 40, at: near(-3.2), stream: nil,
                    kind: .exited(LogExited(code: 0, signal: nil))),
                LogEntry(
                    seq: 41, at: near(-2.0), stream: nil,
                    kind: .started(LogStarted(pid: 48_213))),
                LogEntry(seq: 42, at: near(-1.5), stream: nil, kind: .unavailable),
                line(43, near(-0.8), .stdout, "server started"),
                LogEntry(
                    seq: 44, at: near(0.4), stream: .stdout,
                    kind: .truncated(LogTruncated(bytes: 1_572_864))),
                line(45, near(0.5), .stdout, "accepted connection from 127.0.0.1:61022"),
                LogEntry(
                    seq: 46, at: near(1.25), stream: nil,
                    kind: .exited(LogExited(code: nil, signal: 9))),
            ]
        }
    }
#endif
