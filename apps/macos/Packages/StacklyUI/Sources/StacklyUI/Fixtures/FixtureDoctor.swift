#if DEBUG
    import Foundation
    import StacklyClient

    /// The doctor reports, findings, and bundle preview of the Diagnostics fixtures (spec
    /// 0034, AC-21; spec 0068), built only from the generated contract types. The instances
    /// are the ones of `connected-mixed` (pg17-dev, orders-db, mysql-erp), so a route opens a
    /// real fixture instance. No text holds a secret, and every path uses `~`.
    public nonisolated enum FixtureDoctor {
        /// When every fixture report finished: 2 minutes before ``FixtureClock/now``, so a
        /// report is never older than 5 minutes and Diagnostics does not run again.
        public static let finishedAt = FixtureClock.minutesAgo(2)

        /// pg17-dev, instance 1 of `connected-mixed`.
        public static let pg17 = instance(1, "pg17-dev")
        /// orders-db, instance 2 of `connected-mixed`.
        public static let ordersDb = instance(2, "orders-db")
        /// mysql-erp, instance 4 of `connected-mixed`.
        public static let mysqlErp = instance(4, "mysql-erp")

        // MARK: - Reports

        /// The mixed report: an Error port conflict with Use Port, a Warning drift with
        /// Review, and every other check OK.
        public static var mixed: DoctorReport {
            report(
                checks: okChecks(before: .endpoints) + [
                    check(
                        .endpoints, "Endpoints", "1 conflict across 6 configured endpoints.",
                        [portInUse])
                ] + okChecks(between: .endpoints, and: .configDrift) + [
                    check(.configDrift, "Configuration revisions", "1 drifted file.", [configDrift])
                ] + okChecks(after: .configDrift),
                errors: 1, warnings: 1)
        }

        /// Every check OK.
        public static var allOK: DoctorReport {
            report(checks: okChecks(before: nil), errors: 0, warnings: 0)
        }

        /// The daemon in recovery mode: the registry check reports it and the checks that
        /// need the registry report `check_unavailable`.
        public static var recovery: DoctorReport {
            let registry = check(
                .registry, "Registry", "The registry is in recovery mode.",
                [
                    finding(
                        .recoveryMode, .error, title: "Registry in recovery mode",
                        explanation:
                            "stacklyd opened the registry read only because a migration did not finish. Instances keep their data; nothing can change until recovery ends."
                    )
                ])
            let unavailable: [DoctorCheck] = [
                (.runtimes, "Runtimes"), (.endpoints, "Endpoints"), (.processes, "Processes"),
            ].map { id, title in
                check(
                    id, title, "Not checked in recovery mode.",
                    [
                        finding(
                            .checkUnavailable, .warning, title: "\(title) check unavailable",
                            explanation:
                                "This check needs the registry, which is in recovery mode.",
                            details: [DoctorDetail(key: "reason", value: "recovery_mode")])
                    ])
            }
            return report(
                checks: [
                    ok(.daemon, "stacklyd", "stacklyd 0.9.0 · protocol 3.5 · launchd"), registry,
                    ok(
                        .permissions, "Permissions",
                        "All Stackly folders are private and writable by you."),
                    ok(.diskSpace, "Disk space", "41% used · 410 GB free"),
                ] + unavailable,
                errors: 1, warnings: 3, recovery: true)
        }

        // MARK: - Findings

        /// Port conflict on mysql-erp: mysqld of Homebrew holds 3306, Use Port 3307.
        public static var portInUse: DoctorFinding {
            finding(
                .portInUse, .error, title: "Port conflict · mysql-erp",
                explanation:
                    "127.0.0.1:3306 is held by mysqld (PID 812, ~/homebrew/bin/mysqld). Not owned by Stackly.",
                subject: "mysql", instance: mysqlErp,
                details: [
                    DoctorDetail(key: "endpoint", value: "mysql"),
                    DoctorDetail(key: "address", value: "127.0.0.1"),
                    DoctorDetail(key: "port", value: "3306"),
                    DoctorDetail(key: "suggestedPort", value: "3307"),
                ],
                portOwner: PortOwner(
                    kind: .process,
                    process: ProcessRef(
                        pid: 812, startedAt: FixtureClock.daysAgo(1),
                        executable: "~/homebrew/bin/mysqld", sameUser: true),
                    instance: nil),
                action: DoctorAction(
                    kind: .changePort, instanceId: mysqlErp.id, operationId: nil,
                    endpoint: "mysql", port: 3307))
        }

        /// A drifted `postgresql.conf` of orders-db, with Review.
        public static var configDrift: DoctorFinding {
            finding(
                .configDrift, .warning, title: "Config changed outside Stackly · orders-db",
                explanation:
                    "postgresql.conf differs from the revision Stackly rendered. Stackly renders nothing until you keep or resolve it.",
                instance: ordersDb,
                details: [DoctorDetail(key: "file", value: "postgresql.conf")],
                action: DoctorAction(
                    kind: .reviewConfig, instanceId: ordersDb.id, operationId: nil,
                    endpoint: nil, port: nil))
        }

        /// `pid_reused` on pg17-dev, with the evidence of every identity check.
        public static var pidReused: DoctorFinding {
            let checks: [(IdentityCheckName, IdentityCheckResult)] = [
                (.bootMatches, .passed), (.pidExists, .passed), (.startMatches, .failed),
                (.executableMatches, .failed), (.userMatches, .passed),
                (.sessionMatches, .notChecked), (.holdsLock, .notChecked),
            ]
            return finding(
                .pidReused, .error, title: "PID reused · pg17-dev",
                explanation:
                    "PID 4242 now belongs to /bin/sleep, not the process Stackly recorded. Stackly leaves both alone.",
                instance: pg17,
                evidence: DoctorEvidence(
                    identity: IdentityEvidence(
                        checks: checks.map { IdentityCheckEntry(name: $0.0, result: $0.1) },
                        pid: 4242, lock: .free, stopped: false, error: nil),
                    recorded: ProcessRef(
                        pid: 4242, startedAt: FixtureClock.daysAgo(1),
                        executable:
                            "~/Library/Application Support/Stackly/runtimes/postgresql/17.6/bin/postgres",
                        sameUser: true),
                    current: ProcessRef(
                        pid: 4242, startedAt: FixtureClock.minutesAgo(3),
                        executable: "/bin/sleep", sameUser: true),
                    observedAt: finishedAt),
                action: DoctorAction(
                    kind: .showInstance, instanceId: pg17.id, operationId: nil, endpoint: nil,
                    port: nil))
        }

        /// `start_failed` on pg17-dev, with the last output and the PostgreSQL remedy for an
        /// old `postmaster.pid`.
        public static var startFailed: DoctorFinding {
            let pidFile =
                "~/Library/Application Support/Stackly/instances/pg17-dev/data/postmaster.pid"
            return finding(
                .startFailed, .error, title: "Start failed · pg17-dev",
                explanation: "pg17-dev exited with code 1 while it started.",
                instance: pg17,
                details: [
                    DoctorDetail(key: "failureCode", value: "process_exited"),
                    DoctorDetail(key: "exitCode", value: "1"),
                ],
                excerpt: [
                    #"FATAL:  lock file "postmaster.pid" already exists"#,
                    "HINT:  Is another postmaster (PID 4242) running in data directory?",
                ],
                remedy: DoctorRemedy(
                    provider: "official/postgresql", revision: "2026.11.1",
                    name: "stale_postmaster_pid",
                    title: "PostgreSQL found an old postmaster.pid",
                    steps: [
                        RemedyStep(
                            text:
                                "Stackly never removes this file, because it also guards the data folder against a second server.",
                            command: nil, path: nil),
                        RemedyStep(
                            text: "Check what holds the PID before you change anything.",
                            command: "ps -p 4242 -o pid,comm", path: nil),
                        RemedyStep(
                            text:
                                "If the PID isn’t a PostgreSQL server for this data folder, remove the file yourself, then start the instance again.",
                            command: nil, path: pidFile),
                    ],
                    pidFile: PidFileEvidence(
                        path: pidFile, pid: 4242, state: .otherProcess,
                        process: ProcessRef(
                            pid: 4242, startedAt: FixtureClock.minutesAgo(3),
                            executable: "/bin/sleep", sameUser: true))),
                action: DoctorAction(
                    kind: .showInstance, instanceId: pg17.id, operationId: nil, endpoint: nil,
                    port: nil))
        }

        // MARK: - Bundle

        /// The preview of a bundle with every kind of entry.
        public static var preview: BundlePreview {
            let entries: [(String, BundleEntryKind, String, UInt64, UInt64?, Bool)] = [
                (
                    "README.txt", .readme, "What this bundle holds and what it leaves out.", 2_048,
                    nil, false
                ),
                ("doctor.json", .doctor, "The doctor report of this preview.", 18_320, nil, false),
                (
                    "system.json", .system,
                    "Stackly and protocol versions, macOS, the launch context, capacity, and the Stackly folders.",
                    1_460, nil, false
                ),
                (
                    "instances.json", .instances,
                    "Every instance with its provider, runtime, state, last failure, and endpoints; secrets appear only as references.",
                    9_210, nil, false
                ),
                (
                    "runtimes.json", .runtimes,
                    "Every runtime with its version, source, and digest.", 1_980, nil, false
                ),
                (
                    "operations.json", .operations,
                    "The 100 newest operations and every one that needs recovery.", 48_100, nil,
                    false
                ),
                (
                    "logs/stacklyd.log", .daemonLog, "The last 2000 lines of the stacklyd log.",
                    412_000, 2_000, true
                ),
                (
                    "logs/instances/pg17-dev.log", .instanceLog, "The last 500 lines of pg17-dev.",
                    61_400, 500, false
                ),
            ]
            return BundlePreview(
                previewId: FixtureID.uuid(kind: FixtureID.operation, number: 90),
                createdAt: FixtureClock.now, expiresAt: FixtureClock.now.addingTimeInterval(900),
                suggestedFileName: "stackly-diagnostics-20261018T000000Z.zip",
                entries: entries.map {
                    BundleEntry(
                        path: $0.0, kind: $0.1, description: $0.2, bytes: $0.3, lines: $0.4,
                        truncated: $0.5)
                },
                totalBytes: entries.reduce(0) { $0 + $1.3 }, zipBytes: 101_233, recovery: false)
        }

        /// The error of a preview the daemon could not build.
        public static var bundleFailed: ErrorBody {
            ErrorBody(
                code: "diagnostics.bundle_failed", category: .failed,
                message: "the bundle could not be built: no space left on the data volume",
                retryable: true, resource: nil, fields: nil, hint: nil, details: nil,
                operationId: nil, rule: nil, items: nil)
        }

        /// The error of a doctor run that failed.
        public static var runFailed: ErrorBody {
            ErrorBody(
                code: "ipc.timeout", category: .timeout,
                message: "no answer from stacklyd within 30 seconds", retryable: true,
                resource: nil, fields: nil, hint: nil, details: nil, operationId: nil,
                rule: nil, items: nil)
        }

        // MARK: - Builders

        private static func instance(_ number: UInt8, _ name: String) -> DoctorInstanceRef {
            DoctorInstanceRef(
                id: FixtureID.uuid(kind: FixtureID.instance, number: number), name: name)
        }

        private static func report(
            checks: [DoctorCheck], errors: UInt64, warnings: UInt64, recovery: Bool = false
        ) -> DoctorReport {
            DoctorReport(
                runId: FixtureID.uuid(kind: FixtureID.operation, number: 91),
                startedAt: finishedAt.addingTimeInterval(-1.25), finishedAt: finishedAt,
                recovery: recovery, scope: nil, checks: checks, errors: errors,
                warnings: warnings)
        }

        private static func check(
            _ id: DoctorCheckId, _ title: String, _ summary: String, _ findings: [DoctorFinding]
        ) -> DoctorCheck {
            let severity: DoctorSeverity =
                findings.contains { $0.severity == .error } ? .error : .warning
            return DoctorCheck(
                id: id, severity: severity, title: title, summary: summary, findings: findings,
                omitted: 0, capacity: nil)
        }

        private static func ok(_ id: DoctorCheckId, _ title: String, _ summary: String)
            -> DoctorCheck
        {
            DoctorCheck(
                id: id, severity: .ok, title: title, summary: summary, findings: [], omitted: 0,
                capacity: nil)
        }

        /// Every daemon check OK, in the daemon's order.
        private static let okRows: [DoctorCheck] = [
            ok(.daemon, "stacklyd", "stacklyd 0.9.0 · protocol 3.5 · launchd"),
            ok(.registry, "Registry", "quick_check passed · schema 12"),
            ok(.permissions, "Permissions", "All Stackly folders are private and writable by you."),
            ok(.diskSpace, "Disk space", "41% used · 410 GB free"),
            ok(.runtimes, "Runtimes", "4 runtimes. Every executable is in place."),
            ok(.providerPackages, "Provider packages", "Every pinned package is readable."),
            ok(.endpoints, "Endpoints", "6 endpoints, no conflict."),
            ok(.processes, "Processes", "Every recorded process is its instance."),
            ok(.instanceFiles, "Instance files", "Every instance has its data and runtime."),
            ok(.instanceFailures, "Start failures", "No instance failed to start."),
            ok(
                .configDrift, "Configuration revisions", "Every rendered file matches its revision."
            ),
            ok(.secrets, "Secrets", "Every secret reference is in the Keychain."),
            ok(.operations, "Operations", "Nothing needs recovery."),
        ]

        private static func okChecks(before id: DoctorCheckId?) -> [DoctorCheck] {
            guard let id, let end = okRows.firstIndex(where: { $0.id == id }) else {
                return okRows
            }
            return Array(okRows[..<end])
        }

        private static func okChecks(between start: DoctorCheckId, and end: DoctorCheckId)
            -> [DoctorCheck]
        {
            guard let from = okRows.firstIndex(where: { $0.id == start }),
                let to = okRows.firstIndex(where: { $0.id == end })
            else { return [] }
            return Array(okRows[(from + 1)..<to])
        }

        private static func okChecks(after id: DoctorCheckId) -> [DoctorCheck] {
            guard let from = okRows.firstIndex(where: { $0.id == id }) else { return [] }
            return Array(okRows[(from + 1)...])
        }

        private static func finding(
            _ code: DoctorFindingCode, _ severity: DoctorSeverity, title: String,
            explanation: String, subject: String? = nil, instance: DoctorInstanceRef? = nil,
            details: [DoctorDetail]? = nil, evidence: DoctorEvidence? = nil,
            portOwner: PortOwner? = nil, excerpt: [String]? = nil, remedy: DoctorRemedy? = nil,
            action: DoctorAction? = nil
        ) -> DoctorFinding {
            DoctorFinding(
                code: code, severity: severity, title: title, explanation: explanation,
                subject: subject, instance: instance, operationId: nil, details: details,
                evidence: evidence, portOwner: portOwner, excerpt: excerpt, remedy: remedy,
                action: action)
        }
    }
#endif
