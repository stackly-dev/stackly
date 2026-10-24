#if DEBUG
    import Foundation
    import StacklyClient

    /// The managed runtime cases of the fixtures (spec 0068 added by spec 0018): the
    /// runtimes `runtime.list` and `runtime.get` answer, the install that has not ended,
    /// and the artifact `runtime.resolve` names. Runtime external stays with feature 56.
    /// Every value comes from the generated contract types, every time from
    /// ``FixtureClock``, and every ID from ``FixtureID``: a runtime case has the number of
    /// the instance case that uses it, so `usedBy` lines up with
    /// ``FixtureScenario/instanceCases``. The values are data for a scenario to answer
    /// with; the runtime pages of milestone 4 attach them to one.
    public nonisolated enum FixtureRuntime {
        /// The numbers of the installed runtime cases: the runtimes of `pg17-dev`,
        /// `mysql-erp`, and `queue`, and `12`, which nothing uses.
        public static let installedNumbers: [UInt8] = [1, 4, 5, 12]
        /// The number of the runtime being installed: the one `orders-db` waits for, and
        /// the resource of operation case 1.
        public static let installingNumber: UInt8 = 2

        private static let host = HostTarget(os: .macos, arch: .aarch64)

        /// The runtimes `runtime.list` returns, in list order, all installed and managed.
        public static let summaries: [RuntimeSummary] = [
            summary(
                number: 1, software: "postgresql", displayName: "PostgreSQL", version: "17.6",
                line: "17", users: [(1, "pg17-dev")]),
            summary(
                number: 4, software: "mysql", displayName: "MySQL", version: "8.4.6", line: "8.4",
                users: [(4, "mysql-erp")]),
            summary(
                number: 5, software: "nats", displayName: "NATS Server", version: "2.11.9",
                line: "2.11", users: [(5, "queue")]),
            summary(
                number: 12, software: "traefik", displayName: "Traefik", version: "3.7.15",
                line: "3.7", users: []),
        ]

        /// The install of operation case 1, which `runtime.list` returns on its first page.
        public static let installing = InstallingRuntime(
            operationId: FixtureID.uuid(kind: FixtureID.operation, number: 1),
            runtimeId: FixtureID.uuid(kind: FixtureID.runtime, number: installingNumber),
            provider: "official/postgresql", software: "postgresql", displayName: "PostgreSQL",
            version: "18.1", target: host, sha256: digest(installingNumber))

        /// The page `runtime.list` returns: every installed runtime and the running install.
        public static let page = RuntimeListPage(
            items: summaries, next: nil, installing: [installing])

        /// What `runtime.get` returns for the first runtime, with everything the inspector
        /// shows.
        public static let detail = RuntimeDetail(
            summary: summaries[0],
            path:
                "/fixture/Library/Application Support/Stackly/runtimes/postgresql/17.6/macos-aarch64/\(digest(1))",
            artifact: ArtifactInfo(
                catalogId: "postgresql-17.6-r2-macos-aarch64", sha256: digest(1),
                size: 27_984_415, unpackedSize: 79_682_757, format: .tarGz, minimumOs: "13.0",
                build: 2, source: "github.com/stackly-runtimes",
                signature: SignatureInfo(
                    policy: .keepOrAdhoc, adhocSignedAtInstall: false, machOFiles: 210,
                    executables: [
                        ExecutableSignature(path: "bin/postgres", kind: .adhoc, teamId: nil),
                        ExecutableSignature(path: "bin/psql", kind: .adhoc, teamId: nil),
                    ]),
                dependencies: [
                    DependencyInfo(
                        name: "libssl.3.dylib", kind: .bundled,
                        installName: "@rpath/libssl.3.dylib", version: "3.5.9",
                        license: "Apache-2.0", sourceUrl: "https://example.invalid/openssl"),
                    DependencyInfo(
                        name: "libSystem.B.dylib", kind: .system,
                        installName: "/usr/lib/libSystem.B.dylib", version: nil, license: nil,
                        sourceUrl: nil),
                ],
                warnings: []),
            executables: ["bin/postgres", "bin/initdb", "bin/psql", "bin/pg_dump"],
            compatibility: [
                CompatibilityRecordView(
                    scope: .runtime, result: .pass, providerRevision: nil, os: .macos,
                    osVersion: "27.0", osBuild: "26A428", arch: .aarch64,
                    checks: ["launch_agent", "ready", "stop"],
                    knownLimitations: ["provider_integration_not_covered"],
                    summary: "postgresql 17.6 ready in 493 ms, stopped in 184 ms",
                    testedAt: FixtureClock.daysAgo(3),
                    evidence: "runtime/fixture-run/fixture-record", sourceUrl: nil,
                    sha256: digest(1))
            ])

        /// The detail of `summary`: the fixture detail with the summary, the digest, and the
        /// path of that runtime.
        /// - Parameter summary: A runtime of the fixtures.
        public static func detail(of summary: RuntimeSummary) -> RuntimeDetail {
            let digest = summary.artifactSha256 ?? digest(0)
            let artifact = detail.artifact
            return RuntimeDetail(
                summary: summary,
                path:
                    "/fixture/Library/Application Support/Stackly/runtimes/\(summary.software)/\(summary.version)/macos-aarch64/\(digest)",
                artifact: artifact.map {
                    ArtifactInfo(
                        catalogId: $0.catalogId, sha256: digest, size: $0.size,
                        unpackedSize: $0.unpackedSize, format: $0.format,
                        minimumOs: $0.minimumOs, build: $0.build, source: $0.source,
                        signature: $0.signature, dependencies: $0.dependencies,
                        warnings: $0.warnings)
                },
                executables: detail.executables,
                compatibility: detail.compatibility?.map {
                    CompatibilityRecordView(
                        scope: $0.scope, result: $0.result, providerRevision: $0.providerRevision,
                        os: $0.os, osVersion: $0.osVersion, osBuild: $0.osBuild, arch: $0.arch,
                        checks: $0.checks, knownLimitations: $0.knownLimitations,
                        summary: $0.summary, testedAt: $0.testedAt, evidence: $0.evidence,
                        sourceUrl: $0.sourceUrl, sha256: digest)
                })
        }

        /// What `runtime.resolve` returns for the runtime being installed.
        public static let resolved = ResolvedArtifact(
            provider: "official/postgresql", software: "postgresql",
            artifactId: "postgresql-18.1-r1-macos-aarch64", version: "18.1", line: "18",
            target: host, sha256: digest(installingNumber), size: 28_311_215,
            unpackedSize: 80_668_813, format: .tarGz, minOs: "13.0", stable: true,
            support: .experimental, supportReason: .runtimeTested)

        /// The replies a scenario gives to the runtime methods that read: `runtime.list`,
        /// `runtime.get` (the first runtime), and `runtime.resolve`.
        public static let replies: [String: FixtureReply] = [
            "runtime.list": .reply(.runtimeList(page)),
            "runtime.get": .reply(.runtimeGet(detail)),
            "runtime.resolve": .reply(.runtimeResolve(resolved)),
        ]

        /// The artifact digest of runtime case `number`: its number repeated as hex.
        static func digest(_ number: UInt8) -> String {
            String(repeating: String(format: "%02x", number), count: 32)
        }

        private static func summary(
            number: UInt8, software: String, displayName: String, version: String, line: String,
            users: [(UInt8, String)]
        ) -> RuntimeSummary {
            RuntimeSummary(
                id: FixtureID.uuid(kind: FixtureID.runtime, number: number), revision: 1,
                software: software, displayName: displayName, version: version, line: line,
                target: host, installMode: .managed, state: .installed,
                artifactSha256: digest(number), installedAt: FixtureClock.daysAgo(Int(number)),
                lastVerifiedAt: FixtureClock.minutesAgo(1), support: .experimental,
                supportReason: .runtimeTested,
                usedBy: users.map {
                    RuntimeUser(
                        instanceId: FixtureID.uuid(kind: FixtureID.instance, number: $0.0),
                        name: $0.1, retained: false)
                }, dependentRuntimes: [])
        }
    }
#endif
