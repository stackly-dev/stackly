#if DEBUG
    import Foundation
    import StacklyClient

    /// The create preflight case of the fixtures (spec 0068 added by spec 0020): the
    /// answer `instance.preflight` gives when a fixed port is taken by another instance of
    /// this world, with the free port the daemon offers and the command that would do the
    /// same. The New Instance sheet of milestone 3 shows it as its blocked state.
    /// Every value comes from the generated contract types and every ID from ``FixtureID``:
    /// the instance that holds the port is instance case 1, so the row names an instance
    /// the scenarios list. The value is data for a scenario to answer with; the sheet
    /// attaches it to one.
    public nonisolated enum FixtureCreate {
        /// The runtime the draft runs on: runtime case 1, PostgreSQL 17.6.
        public static let runtimeNumber: UInt8 = 1
        /// The instance that already holds the fixed port: instance case 1, `pg17-dev`.
        public static let portHolderNumber: UInt8 = 1
        /// The port the draft asks for, and the one the daemon offers instead.
        public static let requestedPort: UInt16 = 5432
        /// The free port offered for the conflicting row.
        public static let suggestedPort: UInt16 = 5433

        private static let portError = ErrorBody(
            code: "supervisor.endpoint_conflict", category: .conflict,
            message: "port 5432 of endpoint postgres is used by instance pg17-dev",
            retryable: false, resource: nil,
            fields: [
                FieldError(
                    path: "endpoints[0].port", code: "invalid_value",
                    message: "value does not match the expected type or format")
            ], hint: nil,
            details: [
                ErrorDetail(key: "endpoint", value: "postgres"),
                ErrorDetail(key: "port", value: "5432"),
                ErrorDetail(key: "otherInstance", value: "pg17-dev"),
                ErrorDetail(key: "omitted", value: "0"),
            ], operationId: nil, rule: nil, items: nil)

        /// The preflight of a draft whose fixed port is taken: `ready` is false, the
        /// endpoint row is an error that offers a free port, and every other row passes.
        public static let portConflict = CreatePreflight(
            ready: false,
            provider: ResolvedProvider(
                name: "official/postgresql", revision: "2026.10.1",
                manifestSha256: String(repeating: "ab", count: 32)),
            checks: [
                check(
                    "runtime", .runtime, .ok, "Runtime PostgreSQL 17.6",
                    "Verified \(String(repeating: "01", count: 6))"),
                check("name", .name, .ok, "Name available", "orders-db"),
                check(
                    "endpoint.postgres", .endpoint, .error, "Endpoint 127.0.0.1:5432",
                    "In use by pg17-dev", subject: "postgres", suggested: suggestedPort,
                    error: portError),
                check(
                    "endpoint.admin", .endpoint, .ok, "Endpoint 127.0.0.1 · admin",
                    "Port assigned when the instance starts", subject: "admin"),
                check(
                    "data", .data, .ok, "Empty data directory",
                    "instances/<new instance id>/data · created new for this instance"),
                check("disk", .disk, .ok, "Disk space", "41 GB free · needs at least 256 MB"),
                check(
                    "secret.superuserPassword", .secret, .ok,
                    "Superuser password stored in Keychain", "Generated on create, never logged",
                    subject: "superuserPassword"),
            ],
            fields: [
                FieldError(
                    path: "endpoints[0].port", code: "invalid_value",
                    message: "value does not match the expected type or format")
            ],
            command:
                "stackly instance create orders-db --runtime postgresql@17.6 --provider official/postgresql --port postgres=5432 --port admin=auto --secret superuserPassword=generate"
        )

        /// The reply a scenario gives to `instance.preflight`.
        public static let replies: [String: FixtureReply] = [
            "instance.preflight": .reply(.instancePreflight(portConflict))
        ]

        private static func check(
            _ id: String, _ kind: PreflightCheckKind, _ status: PreflightStatus, _ title: String,
            _ detail: String, subject: String? = nil, suggested: UInt16? = nil,
            error: ErrorBody? = nil
        ) -> PreflightCheck {
            PreflightCheck(
                id: id, kind: kind, status: status, title: title, detail: detail,
                subject: subject, suggestedPort: suggested, error: error)
        }
    }
#endif
