#if DEBUG
    import Foundation
    import StacklyClient

    /// The Metrics tab fixtures (spec 0028, *Layar Metrics*; spec 0068): the views of the tab
    /// in each of its states, answered from data with no daemon.
    nonisolated enum FixtureMetrics {
        /// A state of the tab, named like the snapshots.
        enum Case: String, CaseIterable, Sendable {
            /// A full history of a PostgreSQL main process, the cards on Main Process.
            case liveMain = "metrics-live-main"
            /// The same history on Process Tree: over 100 percent CPU, six processes.
            case liveTree = "metrics-live-tree"
            /// A tree where five of six processes were measured, and a data size from a
            /// scan that stopped at its limit.
            case partial = "metrics-partial"
            /// Every number missing, each for its own reason, and a data folder not scanned.
            case unavailable = "metrics-unavailable"
            /// A process list cut at its limit.
            case truncated = "metrics-truncated"
            /// An instance that is stopped.
            case notRunning = "metrics-not-running"
        }

        /// The history of every live fixture.
        static let historyId = "4242-1760950802.000000"

        /// The model of a case before it opened.
        @MainActor
        static func model(_ fixture: Case, instance: UUID) -> InstanceMetricsModel {
            InstanceMetricsModel(
                instanceId: instance, service: service(fixture, instance: instance),
                scope: fixture == .liveTree || fixture == .partial ? .tree : .main)
        }

        /// The service of a case: every read answers the whole reply of the case.
        static func service(_ fixture: Case, instance: UUID) -> InstanceMetricsService {
            InstanceMetricsService { _, _, _ throws(ClientError) in
                reply(fixture, instance: instance)
            }
        }

        /// The reply of a case.
        static func reply(_ fixture: Case, instance: UUID) -> MetricsReply {
            let live = fixture != .notRunning
            let samples: [MetricsSample] =
                switch fixture {
                case .liveMain, .liveTree: history(count: 60)
                case .truncated: history(count: 60, treeCount: 230)
                case .partial: history(count: 60, measuredTree: 5)
                case .unavailable: [warmingUpSample]
                case .notRunning: []
                }
            let rows: [ProcessRow] =
                switch fixture {
                case .liveMain, .liveTree: processes(count: 6)
                case .partial: processes(count: 5)
                case .truncated: processes(count: 12)
                case .unavailable: [unavailableRow]
                case .notRunning: []
                }
            return MetricsReply(
                instanceId: instance, state: live ? .live : .notRunning,
                lifecycle: live ? .running : .stopped, intervalSeconds: 5, capacity: 60,
                historyId: live ? historyId : nil,
                process: live
                    ? MetricsProcess(pid: 4242, startedAt: FixtureClock.minutesAgo(30)) : nil,
                samples: samples, latestProcesses: rows,
                processesTruncated: fixture == .truncated, historyReset: true,
                restartTotal: 2, lastFailure: nil,
                readiness: .fixture(
                    live ? .ready : .unknown, reason: live ? "probe_ok" : "not_running",
                    latencyMs: live ? 3 : nil),
                health: live
                    ? .fixture(.healthy, reason: "probe_ok", latencyMs: 3)
                    : .fixture(.unknown, reason: "not_running"),
                dataUsage: dataUsage(fixture))
        }

        private static func dataUsage(_ fixture: Case) -> DataUsageMetric {
            switch fixture {
            case .partial:
                .scanned(
                    ScannedUsage(
                        bytes: 4_200_000_000, complete: false,
                        scannedAt: FixtureClock.minutesAgo(4)))
            case .unavailable, .notRunning:
                .unavailable(UnavailableContent(reason: .notScanned))
            case .liveMain, .liveTree, .truncated:
                .scanned(
                    ScannedUsage(
                        bytes: 1_234_000_000, complete: true,
                        scannedAt: FixtureClock.minutesAgo(4)))
            }
        }

        // MARK: - Samples

        private static func value(_ cpu: Double) -> NumberMetric {
            .value((cpu * 10).rounded() / 10)
        }

        /// 60 samples with a CPU wave that stays under 100 for the main process and goes over
        /// 100 for the tree.
        private static func history(
            count: Int, treeCount: UInt64 = 6, measuredTree: UInt64? = nil
        ) -> [MetricsSample] {
            (1...count).map { seq in
                let wave = Double((seq * 37) % 23) / 2 + 0.4
                return MetricsSample(
                    seq: UInt64(seq),
                    sampledAt: FixtureClock.secondsAgo((count - seq) * 5),
                    main: ResourceUsage(
                        scope: .main, processCount: 1, measuredCount: 1, cpu: value(wave),
                        memory: .value(81_788_928), diskRead: .value(0), diskWrite: .value(0)),
                    tree: ResourceUsage(
                        scope: .tree, processCount: treeCount,
                        measuredCount: measuredTree ?? treeCount,
                        cpu: value(wave * 9 + 24), memory: .value(405_798_912),
                        diskRead: .value(1_536), diskWrite: .value(16_384)))
            }
        }

        private static var warmingUpSample: MetricsSample {
            func usage(_ scope: UsageScope) -> ResourceUsage {
                ResourceUsage(
                    scope: scope, processCount: 1, measuredCount: 0,
                    cpu: .unavailable(UnavailableContent(reason: .warmingUp)),
                    memory: .unavailable(UnavailableContent(reason: .notPermitted)),
                    diskRead: .unavailable(UnavailableContent(reason: .processExited)),
                    diskWrite: .unavailable(UnavailableContent(reason: .notSupported)))
            }
            return MetricsSample(
                seq: 1, sampledAt: FixtureClock.secondsAgo(2), main: usage(.main),
                tree: usage(.tree))
        }

        // MARK: - Processes

        /// A tree of `count` processes: the main process, a few children, and one grandchild.
        private static func processes(count: Int) -> [ProcessRow] {
            (0..<count).map { index in
                let pid = UInt32(4242 + index)
                return ProcessRow(
                    pid: pid, parentPid: index == 0 ? nil : (index == 4 ? 4243 : 4242),
                    depth: index == 0 ? 0 : (index == 4 ? 2 : 1), name: "postgres",
                    isMain: index == 0, cpu: value(index == 0 ? 0.4 : Double(index) * 1.3),
                    memory: .value(
                        index == 0 ? 81_788_928 : 64_880_000 + UInt64(index) * 1_048_576))
            }
        }

        private static var unavailableRow: ProcessRow {
            ProcessRow(
                pid: 4242, parentPid: nil, depth: 0, name: "postgres", isMain: true,
                cpu: .unavailable(UnavailableContent(reason: .warmingUp)),
                memory: .unavailable(UnavailableContent(reason: .notPermitted)))
        }
    }
#endif
