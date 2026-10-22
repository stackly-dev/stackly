#if DEBUG
    import Foundation
    import StacklyClient

    /// The detail of the fixture instances (spec 0068 as extended by spec 0021): what
    /// `instance.get_detail` and `instance.get_data_usage` answer for each instance case, built
    /// only from the generated contract types.
    ///
    /// The values follow the contract fixtures of `protocols/fixtures/instance/`: PostgreSQL
    /// gets a `uri` and a `socket` string with the password as a secret part, NATS gets a `uri`
    /// without a secret, and the other providers describe no connection string. A case that
    /// needs a secret (billing-db) has the secret `missing`. Nothing here holds a secret value.
    public nonisolated enum FixtureDetail {
        /// The home folder of the fixture person; no real home starts with it.
        public static let home = "/Users/stackly-fixture"

        /// The detail of `summary`.
        ///
        /// - Parameters:
        ///   - summary: The instance.
        ///   - packageReadable: Whether the pinned package can be read; `false` gives the detail
        ///     of the unreadable package (no display name, no connection).
        public static func detail(of summary: InstanceSummary, packageReadable: Bool = true)
            -> InstanceDetail
        {
            let provider = providerTitle(of: summary)
            return InstanceDetail(
                instanceId: summary.id, revision: summary.revision,
                provider: DetailProvider(
                    name: summary.provider.name, revision: summary.provider.revision,
                    manifestSha256: summary.provider.manifestSha256,
                    displayName: packageReadable ? provider : nil,
                    packageReadable: packageReadable),
                runtime: DetailRuntime(
                    id: summary.runtimeId, version: summary.runtimeVersion,
                    target: HostTarget(os: .macos, arch: .aarch64), installMode: .managed,
                    artifactSha256: digest(of: summary)),
                dataPath:
                    "\(home)/Library/Application Support/Stackly/instances/\(summary.name)/data",
                startup: .manual,
                secrets: secrets(of: summary),
                dependents: dependents(of: summary),
                connections: packageReadable ? connections(of: summary) : [])
        }

        /// The data size of `summary`: complete and measured five minutes before
        /// ``FixtureClock/now``, and incomplete for the second case.
        public static func usage(of summary: InstanceSummary) -> DataUsage {
            let number = UInt64(summary.id.uuid.15)
            return DataUsage(
                bytes: number * 123_456_789, complete: number != 2,
                scannedAt: FixtureClock.minutesAgo(5))
        }

        // MARK: - Pieces

        private static func providerTitle(of summary: InstanceSummary) -> String {
            switch summary.provider.name {
            case "official/postgresql": "PostgreSQL"
            case "official/mysql": "MySQL"
            case "official/nats": "NATS"
            case "official/valkey": "Valkey"
            case "official/clickhouse": "ClickHouse"
            default: summary.software
            }
        }

        private static func digest(of summary: InstanceSummary) -> String {
            let hex = String(format: "%02x", Int(summary.id.uuid.15))
            return String(repeating: hex + "c0ffee", count: 8).prefix(64).description
        }

        private static func reference(of summary: InstanceSummary, field: String) -> String {
            "local:instance/\(summary.id.uuidString.lowercased())/\(field)"
        }

        private static func secrets(of summary: InstanceSummary) -> [DetailSecret] {
            guard summary.provider.name == "official/postgresql" else { return [] }
            var status = SecretStatus.present
            var reference = reference(of: summary, field: "superuser-password")
            if case .needsSecret(let blocker) = summary.blocker,
                let item = blocker.references?.first
            {
                status = .missing
                reference = item.reference
            }
            return [
                DetailSecret(
                    field: "superuserPassword", title: "Superuser password",
                    reference: reference, status: status)
            ]
        }

        private static func dependents(of summary: InstanceSummary) -> [DetailDependent] {
            guard summary.id.uuid.15 == 1 else { return [] }
            return [
                DetailDependent(
                    environmentId: FixtureID.uuid(kind: 5, number: 1), name: "erp-dev",
                    ownership: .dedicated, leaseActive: true),
                DetailDependent(
                    environmentId: FixtureID.uuid(kind: 5, number: 2), name: "reporting",
                    ownership: .shared, leaseActive: false),
            ]
        }

        private static func connections(of summary: InstanceSummary) -> [ConnectionString] {
            guard let port = summary.endpoints?.first?.port else {
                return notAssigned(of: summary)
            }
            switch summary.provider.name {
            case "official/postgresql":
                let secret = secrets(of: summary).first?.reference ?? ""
                return [
                    postgres(
                        "uri", "Connection string", secret: secret,
                        head: "postgresql://postgres:", tail: "@127.0.0.1:\(port)/postgres"),
                    postgres(
                        "socket", "Unix socket", secret: secret, head: "postgresql://postgres:",
                        tail:
                            "@/postgres?host=\(home)/Library/Application%20Support/Stackly/run/ab12cd34&port=\(port)"
                    ),
                ]
            case "official/nats":
                return [
                    ConnectionString(
                        name: "uri", title: "Client URL", state: .available, reason: nil,
                        masked: "nats://127.0.0.1:\(port)",
                        parts: [
                            ConnectionPart(
                                kind: .text, text: "nats://127.0.0.1:\(port)", secret: nil)
                        ],
                        portMayChange: false)
                ]
            default:
                return []
            }
        }

        private static func notAssigned(of summary: InstanceSummary) -> [ConnectionString] {
            guard summary.provider.name == "official/nats" else { return [] }
            return [
                ConnectionString(
                    name: "uri", title: "Client URL", state: .unavailable,
                    reason: .portNotAssigned, masked: nil, parts: [], portMayChange: true)
            ]
        }

        private static func postgres(
            _ name: String, _ title: String, secret: String, head: String, tail: String
        ) -> ConnectionString {
            ConnectionString(
                name: name, title: title, state: .available, reason: nil,
                masked: head + "****" + tail,
                parts: [
                    ConnectionPart(kind: .text, text: head, secret: nil),
                    ConnectionPart(
                        kind: .secret, text: nil,
                        secret: ConnectionSecretPart(field: "superuserPassword", reference: secret)),
                    ConnectionPart(kind: .text, text: tail, secret: nil),
                ], portMayChange: false)
        }
    }

    /// The start that failed because another process holds a port (spec 0068, *Kasus yang
    /// menunggu DTO*, answered by spec 0021): an instance that ended `failed` with the code
    /// `endpoint_in_use` and the output of the process, and the operation and the error the
    /// daemon gave for the click.
    ///
    /// Like ``FixtureCreate`` it is data for a page rig or a scenario to answer with; it joins
    /// no scenario by itself, so the instance counts of the existing scenarios stay.
    public nonisolated enum FixtureStartConflict {
        /// The case number of the instance.
        public static let number: UInt8 = 12

        /// The instance: PostgreSQL 17.6 on port 5432, which instance case 1 holds.
        public static let instance: InstanceSummary = .fixture(
            number: number, name: "stock-db", port: 5432, lifecycle: .failed, desired: .running,
            lastFailure: InstanceLastFailure(
                code: "endpoint_in_use", at: FixtureClock.minutesAgo(1), exitCode: nil,
                signal: nil,
                excerpt: [
                    "FATAL: could not bind IPv4 address \"127.0.0.1\": Address already in use",
                    "HINT: Is another postmaster already running on port 5432?",
                ]))

        /// The refusal of the daemon to the click on Start.
        public static let error = ErrorBody(
            code: "supervisor.endpoint_conflict", category: .conflict,
            message: "port 5432 of endpoint database is used by instance pg17-dev",
            retryable: false, resource: nil, fields: nil, hint: nil,
            details: [
                ErrorDetail(key: "endpoint", value: "database"),
                ErrorDetail(key: "port", value: "5432"),
                ErrorDetail(key: "otherInstance", value: "pg17-dev"),
            ], operationId: nil, rule: nil, items: nil)
    }
#endif
