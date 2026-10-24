#if DEBUG
    import Foundation
    import StacklyClient

    /// The create form and the preflight answers of the New Instance sheet in the fixtures
    /// (spec 0020, spec 0068): built from the generated contract types, with the values of
    /// the database runtime of the scenarios (runtime case 1) and the endpoints of
    /// ``FixtureCreate/portConflict``.
    /// The values are data for a rig to answer with; the sheet attaches them to nothing.
    public nonisolated enum FixtureCreateForm {
        /// The form of runtime case 1: a client endpoint, an admin endpoint, a secret, two
        /// basic fields and two advanced ones.
        public static let database = CreateForm(
            provider: CreateProvider(
                name: "official/postgresql", revision: "2026.10.1",
                manifestSha256: String(repeating: "ab", count: 32), displayName: "PostgreSQL",
                category: .database, source: .bundledCatalog),
            runtime: FixtureRuntime.summaries[0], suggestedName: "postgresql-17",
            dataPath: "instances/<new instance id>/data",
            endpoints: [
                CreateEndpoint(
                    name: "postgres", transport: .tcp, purpose: .client, host: "127.0.0.1",
                    defaultPort: FixtureCreate.requestedPort, socketName: nil),
                CreateEndpoint(
                    name: "admin", transport: .tcp, purpose: .admin, host: "127.0.0.1",
                    defaultPort: 5_444, socketName: nil),
                CreateEndpoint(
                    name: "socket", transport: .unix, purpose: .client, host: nil,
                    defaultPort: nil, socketName: ".s.PGSQL.${endpoint.postgres.port}"),
            ],
            fields: [
                FormField(
                    key: "superuserPassword", title: "Superuser password",
                    description: "The password of the superuser role.", kind: .text,
                    required: true, secret: true, printableOnly: true, managed: false,
                    visibility: .basic, apply: .immutableAfterInit, default: nil, minLength: 8,
                    maxLength: 128, minimum: nil, maximum: nil, choices: [], minItems: nil,
                    maxItems: nil, itemKey: nil, items: []),
                FormField(
                    key: "logStatement", title: "Log statements", description: nil,
                    kind: .choice, required: false, secret: false, printableOnly: false,
                    managed: false, visibility: .basic, apply: .reload,
                    default: .text("none"), minLength: nil, maxLength: nil, minimum: nil,
                    maximum: nil, choices: ["none", "ddl", "mod", "all"], minItems: nil,
                    maxItems: nil, itemKey: nil, items: []),
                FormField(
                    key: "timezone", title: "Time zone", description: nil, kind: .text,
                    required: false, secret: false, printableOnly: false, managed: false,
                    visibility: .basic, apply: .restart, default: .text("UTC"), minLength: 1,
                    maxLength: 64, minimum: nil, maximum: nil, choices: [], minItems: nil,
                    maxItems: nil, itemKey: nil, items: []),
                FormField(
                    key: "maxConnections", title: "Max connections", description: nil,
                    kind: .integer, required: false, secret: false, printableOnly: false,
                    managed: false, visibility: .advanced, apply: .restart,
                    default: .integer(100), minLength: nil, maxLength: nil, minimum: 5,
                    maximum: 1_000, choices: [], minItems: nil, maxItems: nil, itemKey: nil,
                    items: []),
                FormField(
                    key: "autovacuum", title: "Autovacuum", description: nil, kind: .boolean,
                    required: false, secret: false, printableOnly: false, managed: false,
                    visibility: .advanced, apply: .restart, default: .boolean(true),
                    minLength: nil, maxLength: nil, minimum: nil, maximum: nil, choices: [],
                    minItems: nil, maxItems: nil, itemKey: nil, items: []),
            ],
            capabilities: [
                CapabilityStatus(capability: .multiInstance, supported: true),
                CapabilityStatus(capability: .persistence, supported: true),
            ])

        /// The preflight of a draft that is ready: every row passes, the Auto endpoint says
        /// its port comes at start, and the secret is generated.
        public static let ready = CreatePreflight(
            ready: true, provider: FixtureCreate.portConflict.provider,
            checks: FixtureCreate.portConflict.checks.map { check in
                guard check.status == .error else { return check }
                return PreflightCheck(
                    id: check.id, kind: check.kind, status: .ok,
                    title: check.title, detail: "Free on IPv4 and IPv6 loopback",
                    subject: check.subject, suggestedPort: nil, error: nil)
            }, fields: [],
            command:
                "stackly instance create postgresql-17 --runtime postgresql@17.6 --provider official/postgresql --port postgres=5432 --port admin=5444 --secret superuserPassword=generate --start"
        )

        /// The preflight of a draft with a warning: the port is also in use on the other
        /// loopback family.
        public static let caution = CreatePreflight(
            ready: true, provider: ready.provider,
            checks: ready.checks.map { check in
                guard check.id == "endpoint.postgres" else { return check }
                return PreflightCheck(
                    id: check.id, kind: check.kind, status: .caution, title: check.title,
                    detail:
                        "Also in use on [::1]:5432; clients that connect to localhost may reach that server",
                    subject: check.subject, suggestedPort: nil, error: nil)
            }, fields: [], command: ready.command)

        /// The replies a scenario gives to the create methods: the form, a ready preflight, and
        /// the references Copy from offers.
        public static let replies: [String: FixtureReply] = [
            "instance.get_create_form": .reply(.instanceGetCreateForm(database)),
            "instance.preflight": .reply(.instancePreflight(ready)),
            "secret.list": .reply(
                .secretList(
                    ListSecretsReply(
                        items: secrets,
                        store: SecretStoreStatus(
                            backend: .fake, state: .available, target: nil)))),
        ]

        /// A reference of the Keychain that Copy from offers: the secret of an instance of
        /// the scenarios.
        public static let copyFromReference = "local:instance/\(instanceOne)/superuser-password"

        /// The `secret.list` rows of the fixtures: one of instance case 1, one the person
        /// registered, and one Stackly keeps for itself.
        public static let secrets: [SecretRefSummary] = [
            row(
                copyFromReference, owner: .instance, ownerId: instanceID,
                purpose: "superuser-password"),
            row("local:user/shared-token", owner: .user, ownerId: nil, purpose: "user"),
            row("local:stackly/internal", owner: .user, ownerId: nil, purpose: "user"),
        ]

        private static let instanceID = FixtureID.uuid(
            kind: FixtureID.instance, number: FixtureCreate.portHolderNumber)
        private static let instanceOne = instanceID.uuidString.lowercased()

        private static func row(
            _ reference: String, owner: SecretOwnerKind, ownerId: UUID?, purpose: String
        ) -> SecretRefSummary {
            SecretRefSummary(
                reference: reference, ownerKind: owner, ownerId: ownerId, purpose: purpose,
                status: .present, statusObservedAt: FixtureClock.minutesAgo(5),
                valueOrigin: nil, valueUpdatedAt: nil, usedBy: nil, revision: 1)
        }
    }
#endif
