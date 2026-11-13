#if DEBUG
    import Foundation
    import StacklyClient

    /// The Configuration tab fixtures (spec 0029, *GUI*; spec 0068): the config view of
    /// PostgreSQL, NATS, and Traefik with the real keys and classes of their packages, the
    /// drift and roll back states, and the edits that give the pending, invalid, and Effective
    /// File states. Everything is built from the generated contract types and answered from
    /// data with no daemon; nothing here holds a secret value, only the reference of one.
    public nonisolated enum FixtureConfiguration {
        /// A state of the tab, named like the snapshots.
        public enum Case: String, CaseIterable, Sendable {
            /// PostgreSQL in sync: a locked secret and set once field, a restart and a reload
            /// field, an Advanced group, and a History of three revisions.
            case postgresql = "config-postgresql"
            /// NATS, which cannot reload: every field says it requires a restart.
            case nats = "config-nats"
            /// Traefik with its `routes` list, keyed by `name`.
            case traefik = "config-traefik"
            /// A PostgreSQL config file edited outside Stackly.
            case drift = "config-drift"
            /// A PostgreSQL revision that never reached ready, with a last known good one.
            case rollBack = "config-roll-back"
        }

        /// What the person typed before the snapshot.
        public enum Edit: Sendable {
            /// Nothing.
            case none
            /// A restart field and a reload field changed.
            case restartAndReload
            /// Only a reload field changed.
            case reloadOnly
            /// A number outside the bounds of its field.
            case invalid
            /// A route added to the Traefik list.
            case addRoute
        }

        /// The instance every view belongs to (instance case 1 of the scenarios).
        public static let instance = FixtureID.uuid(kind: FixtureID.instance, number: 1)

        // MARK: - Model and service

        /// The model of a case, opened, with the edit typed and judged.
        /// - Parameters:
        ///   - fixture: The case.
        ///   - edit: What was typed.
        @MainActor
        public static func model(_ fixture: Case, edit: Edit = .none) async -> ConfigurationModel {
            let model = ConfigurationModel(
                instanceId: instance, service: service(fixture),
                // The pause never ends: the fixture judges the edit itself.
                sleep: { _ in try await Task.sleep(for: .seconds(3_600)) })
            await model.open()
            guard let schema = model.schema else { return model }
            switch edit {
            case .none: break
            case .restartAndReload:
                schema.setText("maxConnections", "300")
                schema.setText("logMinDurationStatementMs", "250")
            case .reloadOnly:
                schema.setText("logStatement", "ddl")
            case .invalid:
                schema.setText("maxConnections", "5000")
            case .addRoute:
                schema.addItem(to: "routes")
                if let item = schema.items("routes").last {
                    schema.setItemText("routes", item: item.id, entry: "name", "docs")
                    schema.setItemText(
                        "routes", item: item.id, entry: "rule", "Host(`docs.localhost`)")
                    schema.setItemText(
                        "routes", item: item.id, entry: "backendUrl", "http://127.0.0.1:4000")
                }
            }
            if edit != .none { await model.validateNow() }
            return model
        }

        /// The Rename sheet of the first fixture instance, with `name` typed.
        /// - Parameters:
        ///   - name: What was typed, or `nil` for the current name.
        ///   - others: The names of the other instances.
        @MainActor
        public static func renameModel(
            typing name: String? = nil, others: Set<String> = ["billing-db", "analytics"]
        ) -> RenameModel {
            let model = RenameModel(
                source: InstanceEditSource(id: instance, name: "pg17-dev"),
                service: service(.postgresql), others: { others })
            if let name { model.name = name }
            return model
        }

        /// The Ports sheet of the first fixture instance, with its endpoint read and a port
        /// typed.
        /// - Parameters:
        ///   - port: The port typed for the `postgres` endpoint, or `nil` for none.
        ///   - taken: Whether the daemon says another Stackly instance holds that port and
        ///     offers 5435.
        ///   - leases: The environments whose lease holds the instance, in the verdict.
        @MainActor
        public static func endpointsModel(
            typing port: String? = nil, taken: Bool = false,
            leases: [InstanceEnvironment]? = nil
        ) async -> EndpointsModel {
            let verdict =
                taken
                ? EndpointValidation(
                    valid: false,
                    problems: [
                        EndpointProblem(
                            endpoint: "postgres", code: "supervisor.endpoint_conflict",
                            suggestedPort: 5_435)
                    ], changes: [], impact: .restart, leases: nil)
                : EndpointValidation(
                    valid: true, problems: [],
                    changes: [
                        EndpointChange(
                            name: "postgres", old: .fixed(5_432),
                            new: .fixed(UInt16(port ?? "") ?? 5_433))
                    ], impact: .restart, leases: leases)
            let model = EndpointsModel(
                source: InstanceEditSource(id: instance, name: "pg17-dev"),
                service: service(.postgresql, endpoints: verdict),
                sleep: { _ in try await Task.sleep(for: .seconds(3_600)) })
            await model.load()
            if let port {
                model.setPortText(port, of: "postgres")
                await model.validateNow()
            }
            return model
        }

        /// The service of a case: a read answers the view of the case and a verdict is worked
        /// out from the fields, the way the daemon's evaluator does for the fixtures.
        /// - Parameters:
        ///   - fixture: The case.
        ///   - endpoints: What the dry run of a port change answers.
        public static func service(
            _ fixture: Case,
            endpoints: EndpointValidation = EndpointValidation(
                valid: true, problems: [], changes: [], impact: .restart, leases: nil)
        ) -> InstanceConfigService {
            let view = view(fixture)
            return InstanceConfigService(
                get: { _ throws(ClientError) in view },
                validate: { _, _, change throws(ClientError) in
                    verdict(of: change, on: view, fixture: fixture)
                },
                validateEndpoints: { _, _, _ throws(ClientError) in endpoints },
                send: { _, _ in .notSent(ClientErrorCatalog.notConnected()) },
                updates: { _ in AsyncStream { $0.finish() } },
                readOperation: { _ in nil })
        }

        // MARK: - Views

        /// The config view of a case.
        public static func view(_ fixture: Case) -> InstanceConfigView {
            switch fixture {
            case .postgresql: postgresql(drift: nil, active: 3, ready: true)
            case .drift: postgresql(drift: drift, active: 3, ready: true)
            case .rollBack: postgresql(drift: nil, active: 4, ready: false)
            case .nats: nats
            case .traefik: traefik
            }
        }

        private static let drift = ConfigDrift(
            observedAt: FixtureClock.minutesAgo(4),
            files: [
                DriftFile(
                    path: "postgresql.conf", kind: .modified,
                    diff: [
                        StacklyClient.DiffLine(kind: .context, text: "cluster_name = pg17-dev"),
                        StacklyClient.DiffLine(kind: .removed, text: "max_connections = 200"),
                        StacklyClient.DiffLine(kind: .added, text: "max_connections = 400"),
                        StacklyClient.DiffLine(kind: .context, text: "shared_buffers = 128MB"),
                    ])
            ])

        private static func postgresql(drift: ConfigDrift?, active: UInt64, ready: Bool)
            -> InstanceConfigView
        {
            let lastKnownGood = ready ? active : active - 1
            let history = (0..<active).prefix(4).map { offset -> ConfigRevisionSummary in
                let number = active - offset
                let connections = [200, 150, 100, 100][Int(offset)]
                return revision(
                    number, origin: number == 1 ? .created : .applied,
                    minutesAgo: 6 + Int(offset) * 90,
                    ready: number != active || ready, active: offset == 0,
                    good: number == lastKnownGood, values: pgValues(connections: connections),
                    port: 5_432)
            }
            let values = pgValues(connections: 200)
            return InstanceConfigView(
                revision: history[0], lastKnownGood: lastKnownGood, fields: pgFields,
                endpoints: [endpoint("postgres", 5_432, bound: 5_432)], history: history,
                effectiveFiles: [file("postgresql.conf", render(.postgresql, values))],
                effectiveFilesNote: nil, drift: drift, reloadSupported: true)
        }

        private static var nats: InstanceConfigView {
            let values = JSONObject([
                "serverName": .string("events"), "jetstream": .bool(true),
                "clientPassword": .string("secret:events/clientPassword"),
                "jetstreamMaxMemoryMiB": .number(1_024),
                "jetstreamMaxFileMiB": .number(10_240),
                "maxPayloadKiB": .number(1_024),
            ])
            let current = revision(
                1, origin: .created, minutesAgo: 30, ready: true, active: true, good: true,
                values: values, port: 4_222)
            return InstanceConfigView(
                revision: current, lastKnownGood: 1, fields: natsFields,
                endpoints: [endpoint("client", 4_222, bound: 4_222)], history: [current],
                effectiveFiles: [file("nats.conf", render(.nats, values))],
                effectiveFilesNote: nil, drift: nil, reloadSupported: false)
        }

        private static var traefik: InstanceConfigView {
            let values = JSONObject([
                "logLevel": .string("ERROR"), "dynamicDir": .string("dynamic"),
                "routes": .array([
                    route("api", "Host(`api.localhost`)", "http://127.0.0.1:3000"),
                    route("web", "Host(`web.localhost`)", "http://127.0.0.1:3001"),
                ]),
            ])
            let current = revision(
                2, origin: .applied, minutesAgo: 12, ready: true, active: true, good: true,
                values: values, port: 8_080)
            return InstanceConfigView(
                revision: current, lastKnownGood: 2, fields: traefikFields,
                endpoints: [endpoint("web", 8_080, bound: 8_080)],
                history: [
                    current,
                    revision(
                        1, origin: .created, minutesAgo: 600, ready: true, active: false,
                        good: false, values: JSONObject(["logLevel": .string("ERROR")]),
                        port: 8_080),
                ],
                effectiveFiles: [file("routes.yaml", render(.traefik, values))],
                effectiveFilesNote: nil, drift: nil, reloadSupported: true)
        }

        // MARK: - Pieces

        private static func pgValues(connections: Int) -> JSONObject {
            JSONObject([
                "superuserPassword": .string("secret:pg17-dev/superuserPassword"),
                "maxConnections": .number(Double(connections)),
                "sharedBuffersMb": .number(128), "logStatement": .string("none"),
                "logMinDurationStatementMs": .number(-1),
            ])
        }

        private static func route(_ name: String, _ rule: String, _ backend: String) -> JSONValue {
            .object(["name": .string(name), "rule": .string(rule), "backendUrl": .string(backend)])
        }

        private static func endpoint(_ name: String, _ port: UInt16, bound: UInt16?)
            -> ConfigEndpoint
        {
            ConfigEndpoint(
                name: name, transport: .tcp, host: "127.0.0.1", request: .fixed(port),
                boundPort: bound, changeable: true)
        }

        private static func revision(
            _ number: UInt64, origin: ConfigRevisionOrigin, minutesAgo: Int, ready: Bool,
            active: Bool, good: Bool, values: JSONObject, port: UInt16
        ) -> ConfigRevisionSummary {
            ConfigRevisionSummary(
                number: number, createdAt: FixtureClock.minutesAgo(minutesAgo), origin: origin,
                sourceNumber: nil, readyAt: ready ? FixtureClock.minutesAgo(minutesAgo - 1) : nil,
                isActive: active, isLastKnownGood: good, values: values,
                endpoints: [EndpointRequest(name: "postgres", port: .fixed(port))])
        }

        private static func file(_ path: String, _ lines: [String]) -> EffectiveFile {
            EffectiveFile(
                path: path, content: lines.joined(separator: "\n") + "\n", changedLines: [],
                preview: false, portNote: nil)
        }

        // MARK: - Fields

        private static func field(
            _ key: String, _ title: String, _ kind: FieldKind, apply: FieldApply?,
            visibility: FieldVisibility = .basic, secret: Bool = false, managed: Bool = false,
            description: String? = nil, default value: SettingValue? = nil,
            minimum: Int64? = nil, maximum: Int64? = nil, choices: [String] = [],
            itemKey: String? = nil, items: [FormItemField] = []
        ) -> FormField {
            FormField(
                key: key, title: title, description: description, kind: kind, required: false,
                secret: secret, printableOnly: secret, managed: managed, visibility: visibility,
                apply: apply, default: value, minLength: nil, maxLength: nil, minimum: minimum,
                maximum: maximum, choices: choices, minItems: nil,
                maxItems: kind == .list ? 64 : nil,
                itemKey: itemKey, items: items)
        }

        private static func locked(_ field: FormField) -> ConfigField {
            ConfigField(
                field: field,
                locked: field.secret || field.managed || field.apply == .immutableAfterInit)
        }

        private static var pgFields: [ConfigField] {
            [
                field(
                    "superuserPassword", "Superuser password", .text, apply: .immutableAfterInit,
                    secret: true, description: "The password of the superuser role."),
                field(
                    "logStatement", "Log statements", .choice, apply: .reload,
                    default: .text("none"), choices: ["none", "ddl", "mod", "all"]),
                field(
                    "maxConnections", "Max connections", .integer, apply: .restart,
                    visibility: .advanced, default: .integer(100), minimum: 5, maximum: 1_000),
                field(
                    "sharedBuffersMb", "Shared buffers (MB)", .integer, apply: .restart,
                    visibility: .advanced, default: .integer(128), minimum: 16, maximum: 16_384),
                field(
                    "logMinDurationStatementMs", "Slow statement threshold (ms)", .integer,
                    apply: .reload, visibility: .advanced, default: .integer(-1), minimum: -1,
                    maximum: 3_600_000),
            ].map(locked)
        }

        private static var natsFields: [ConfigField] {
            [
                field(
                    "serverName", "Server name", .text, apply: nil, managed: true),
                field("jetstream", "JetStream", .boolean, apply: .restart, default: .boolean(true)),
                field(
                    "clientPassword", "Client password", .text, apply: .restart, secret: true,
                    description: "The password of the stackly user that clients sign in with."),
                field(
                    "jetstreamMaxMemoryMiB", "JetStream memory limit (MiB)", .integer,
                    apply: .restart, visibility: .advanced, default: .integer(1_024),
                    minimum: 1, maximum: 1_048_576),
                field(
                    "jetstreamMaxFileMiB", "JetStream file limit (MiB)", .integer,
                    apply: .restart, visibility: .advanced, default: .integer(10_240),
                    minimum: 1, maximum: 1_048_576),
                field(
                    "maxPayloadKiB", "Largest message (KiB)", .integer, apply: .restart,
                    visibility: .advanced, default: .integer(1_024), minimum: 1,
                    maximum: 65_536),
            ].map(locked)
        }

        private static var traefikFields: [ConfigField] {
            func item(_ key: String, _ title: String, description: String? = nil) -> FormItemField {
                FormItemField(
                    key: key, title: title, description: description, kind: .text,
                    required: true, default: nil, minLength: nil, maxLength: nil, minimum: nil,
                    maximum: nil, choices: [])
            }
            return [
                field(
                    "logLevel", "Log level", .choice, apply: .restart, default: .text("ERROR"),
                    choices: ["TRACE", "DEBUG", "INFO", "WARN", "ERROR", "FATAL", "PANIC"]),
                field("dynamicDir", "Dynamic config folder", .text, apply: nil, managed: true),
                field(
                    "routes", "Routes", .list, apply: .reload,
                    description:
                        "Each route sends requests that match its rule to a local backend.",
                    default: .list([]), itemKey: "name",
                    items: [
                        item("name", "Name"),
                        item(
                            "rule", "Rule",
                            description: "A Traefik router rule, such as Host(`api.localhost`)."),
                        item(
                            "backendUrl", "Backend URL",
                            description: "A loopback HTTP URL, such as http://127.0.0.1:3000."),
                    ]),
            ].map(locked)
        }

        // MARK: - Rendering and verdicts

        private static func render(_ fixture: Case, _ values: JSONObject) -> [String] {
            func text(_ value: JSONValue?) -> String {
                switch value {
                case .string(let text)?: text
                case .number(let number)?: String(Int64(number))
                case .bool(let on)?: on ? "true" : "false"
                case .array(let items)?: "\(items.count) items"
                case .object?, .null?, nil: ""
                }
            }
            switch fixture {
            case .traefik:
                var lines = ["http:", "  routers:"]
                if case .array(let routes)? = values.members["routes"] {
                    for case .object(let members) in routes {
                        lines.append("    \(text(members["name"])):")
                        lines.append("      rule: \(text(members["rule"]))")
                        lines.append("      service: \(text(members["backendUrl"]))")
                    }
                }
                return lines
            case .nats:
                return [
                    "server_name: events",
                    "max_payload: \(text(values.members["maxPayloadKiB"]))KiB",
                    "jetstream: \(text(values.members["jetstream"]))",
                ]
            case .postgresql, .drift, .rollBack:
                return [
                    "cluster_name = pg17-dev",
                    "max_connections = \(text(values.members["maxConnections"]))",
                    "shared_buffers = \(text(values.members["sharedBuffersMb"]))MB",
                    "log_statement = \(text(values.members["logStatement"]))",
                    "log_min_duration_statement = \(text(values.members["logMinDurationStatementMs"]))",
                ]
            }
        }

        private static func verdict(
            of change: ConfigChangeRequest, on view: InstanceConfigView, fixture: Case
        ) -> ConfigValidation {
            guard case .settings(let proposed) = change else {
                return ConfigValidation(
                    valid: true, fieldErrors: [], changes: [], endpointChanges: [],
                    impact: .restart, preview: [])
            }
            let active = view.revision.values
            var changes: [SettingChange] = []
            var errors: [FieldError] = []
            var impact = ConfigImpact.reload
            for item in view.fields where item.locked == false {
                let key = item.field.key
                let old = active.members[key]
                let new = proposed.members[key]
                guard old != new else { continue }
                if case .number(let number)? = new, let minimum = item.field.minimum,
                    let maximum = item.field.maximum,
                    number < Double(minimum) || number > Double(maximum)
                {
                    errors.append(
                        FieldError(
                            path: "settings.\(key)", code: "out_of_range",
                            message: "value is out of range"))
                }
                changes.append(contentsOf: diff(key, old, new))
                if item.field.apply != .reload || view.reloadSupported == false {
                    impact = .restart
                }
            }
            guard errors.isEmpty else {
                return ConfigValidation(
                    valid: false, fieldErrors: errors, changes: [], endpointChanges: [],
                    impact: .none, preview: [])
            }
            let before = render(fixture, active)
            let after = render(fixture, proposed)
            let path =
                fixture == .traefik
                ? "routes.yaml" : (fixture == .nats ? "nats.conf" : "postgresql.conf")
            let marked = after.enumerated().compactMap { index, line in
                before.contains(line) ? nil : UInt32(index + 1)
            }
            return ConfigValidation(
                valid: true, fieldErrors: [], changes: changes, endpointChanges: [],
                impact: changes.isEmpty ? .none : impact,
                preview: [
                    EffectiveFile(
                        path: path, content: after.joined(separator: "\n") + "\n",
                        changedLines: marked, preview: true, portNote: nil)
                ])
        }

        private static func diff(_ key: String, _ old: JSONValue?, _ new: JSONValue?)
            -> [SettingChange]
        {
            func text(_ value: JSONValue?) -> String? {
                switch value {
                case .string(let text)?: text
                case .number(let number)?: String(Int64(number))
                case .bool(let on)?: on ? "true" : "false"
                case .array, .object, .null, nil: nil
                }
            }
            if case .array(let after)? = new {
                var known: [String] = []
                if case .array(let before)? = old {
                    known = before.compactMap { name(of: $0) }
                }
                return after.compactMap { value in
                    guard let name = name(of: value), known.contains(name) == false else {
                        return nil
                    }
                    return SettingChange(key: "\(key)[name=\(name)]", old: nil, new: "added")
                }
            }
            return [SettingChange(key: key, old: text(old), new: text(new))]
        }

        private static func name(of value: JSONValue) -> String? {
            if case .object(let members) = value, case .string(let name)? = members["name"] {
                return name
            }
            return nil
        }
    }
#endif
