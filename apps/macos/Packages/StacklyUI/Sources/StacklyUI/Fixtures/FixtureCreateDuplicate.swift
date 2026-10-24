#if DEBUG
    import Foundation
    import StacklyClient

    /// The duplicate form of the New Instance sheet in the fixtures (spec 0024, spec 0068):
    /// the answer `instance.get_duplicate_form` gives for `pg17-dev`, instance case 1, built
    /// from the generated contract types over ``FixtureCreateForm/database``.
    /// Every port asks `auto`, the secret generates, the settings are the ones the source
    /// changed, and the secret reference is the one Copy from offers first. The value is data
    /// for a rig to answer with; the sheet attaches it to nothing.
    public nonisolated enum FixtureCreateDuplicate {
        /// The instance the form copies: instance case 1, `pg17-dev`.
        public static let source = DuplicateSourceRef(
            id: FixtureID.uuid(kind: FixtureID.instance, number: FixtureCreate.portHolderNumber),
            name: "pg17-dev")

        /// The name the duplicate starts with.
        public static let suggestedName = "pg17-dev-copy"

        /// The form of the source: the pinned package of the database runtime, every port
        /// `auto`, the secret `generate` with the reference of the source, and the two
        /// settings the source changed.
        public static let pg17 = DuplicateForm(
            source: DuplicateSource(instanceId: source.id, name: source.name),
            form: CreateForm(
                provider: CreateProvider(
                    name: FixtureCreateForm.database.provider.name,
                    revision: FixtureCreateForm.database.provider.revision,
                    manifestSha256: FixtureCreateForm.database.provider.manifestSha256,
                    displayName: FixtureCreateForm.database.provider.displayName,
                    category: FixtureCreateForm.database.provider.category, source: .pinned),
                runtime: FixtureCreateForm.database.runtime, suggestedName: suggestedName,
                dataPath: FixtureCreateForm.database.dataPath,
                endpoints: FixtureCreateForm.database.endpoints,
                fields: FixtureCreateForm.database.fields,
                capabilities: FixtureCreateForm.database.capabilities),
            draft: PreflightCreateParams(
                name: suggestedName, displayName: nil,
                provider: ProviderSelector(
                    name: FixtureCreateForm.database.provider.name,
                    revision: FixtureCreateForm.database.provider.revision),
                runtimeId: FixtureCreateForm.database.runtime.id,
                endpoints: [
                    EndpointRequest(name: "postgres", port: .auto),
                    EndpointRequest(name: "admin", port: .auto),
                ],
                settings: JSONObject([
                    "logStatement": .string("ddl"), "maxConnections": .number(200),
                ]),
                secrets: [SecretPlan(field: "superuserPassword", source: .generate)],
                start: false),
            secrets: [
                DuplicateSecret(
                    field: "superuserPassword",
                    sourceReference: FixtureCreateForm.copyFromReference)
            ])

        /// The preflight of the draft of ``pg17``: every row passes, both ports are assigned
        /// when the instance starts, and the command is the one the draft is worth.
        public static let ready = CreatePreflight(
            ready: true, provider: FixtureCreate.portConflict.provider,
            checks: FixtureCreateForm.ready.checks.map { check in
                switch check.id {
                case "name":
                    return PreflightCheck(
                        id: check.id, kind: check.kind, status: .ok, title: check.title,
                        detail: suggestedName, subject: check.subject, suggestedPort: nil,
                        error: nil)
                case "endpoint.postgres":
                    return PreflightCheck(
                        id: check.id, kind: check.kind, status: .ok,
                        title: "Endpoint 127.0.0.1 · postgres",
                        detail: "Port assigned when the instance starts", subject: check.subject,
                        suggestedPort: nil, error: nil)
                default:
                    return check
                }
            }, fields: [],
            command:
                "stackly instance create pg17-dev-copy --runtime postgresql@17.6 --provider official/postgresql --revision 2026.10.1 --port postgres=auto --port admin=auto --secret superuserPassword=generate --set logStatement=ddl --set maxConnections=200 --start"
        )

        /// The replies a scenario gives to the duplicate form.
        public static let replies: [String: FixtureReply] = [
            "instance.get_duplicate_form": .reply(.instanceGetDuplicateForm(pg17))
        ]
    }
#endif
