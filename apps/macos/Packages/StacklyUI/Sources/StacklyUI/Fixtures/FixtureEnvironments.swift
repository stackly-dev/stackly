#if DEBUG
    import Foundation
    import StacklyClient

    /// The environments of the fixtures (spec 0044, spec 0068 scenario
    /// `environment-partially-ready`), built only from the generated contract types: the list,
    /// the detail of each, the manifest states, the plans the sheet shows, and the instances
    /// that belong to them.
    public nonisolated enum FixtureEnvironments {
        /// The ID of fixture environment `number`.
        public static func id(_ number: UInt8) -> UUID {
            FixtureID.uuid(kind: FixtureID.environment, number: number)
        }

        /// `erp-development`: four services, one ready (partially ready).
        public static let erpID = id(1)
        /// `billing`: one service, stopped (down).
        public static let billingID = id(2)
        /// `reporting`: one shared service, ready.
        public static let reportingID = id(3)

        /// The manifest of `name` in the fixture home.
        public static func manifest(_ folder: String) -> String {
            "\(FixtureDetail.home)/Projects/\(folder)/stackly.yaml"
        }

        // MARK: - Instances

        private static func instance(_ number: UInt8) -> InstanceSummary {
            FixtureScenario.instanceCases[Int(number) - 1]
        }

        /// The fixture instances with the environments they belong to: `pg17-dev` is the
        /// dedicated `postgres` of erp-development and the shared `db` of reporting, `cache`
        /// the dedicated cache of erp-development, `queue` borrowed by erp-development, and
        /// `mysql-erp` the dedicated database of billing.
        public static let instances: [InstanceSummary] = FixtureScenario.instanceCases.map {
            summary in
            let memberships = memberships[summary.name] ?? []
            guard memberships.isEmpty == false else { return summary }
            return summary.with(environments: memberships)
        }

        private static let memberships: [String: [InstanceEnvironment]] = [
            "pg17-dev": [
                InstanceEnvironment(
                    environmentId: erpID, name: "erp-development", alias: "postgres",
                    ownership: .dedicated),
                InstanceEnvironment(
                    environmentId: reportingID, name: "reporting", alias: "db", ownership: .shared),
            ],
            "cache": [
                InstanceEnvironment(
                    environmentId: erpID, name: "erp-development", alias: "cache",
                    ownership: .dedicated)
            ],
            "queue": [
                InstanceEnvironment(
                    environmentId: erpID, name: "erp-development", alias: "queue",
                    ownership: .shared)
            ],
            "mysql-erp": [
                InstanceEnvironment(
                    environmentId: billingID, name: "billing", alias: "db", ownership: .dedicated)
            ],
        ]

        private static func serviceInstance(_ summary: InstanceSummary) -> ServiceInstance {
            ServiceInstance(
                id: summary.id, name: summary.name, runtimeVersion: summary.runtimeVersion,
                lifecycle: summary.lifecycle, readiness: summary.readiness,
                health: summary.health)
        }

        // MARK: - Summaries and details

        /// The summary of an environment.
        public static func summary(
            id: UUID, name: String, folder: String?, status: EnvironmentStatus, ready: UInt64,
            total: UInt64, revision: UInt64 = 2
        ) -> EnvironmentSummary {
            EnvironmentSummary(
                id: id, name: name, revision: revision, manifestLocation: folder.map(manifest),
                manifestSha256: String(repeating: "d", count: 64), status: status,
                readyCount: ready, serviceCount: total, updatedAt: FixtureClock.minutesAgo(20))
        }

        /// The summaries `env.list` answers, by name.
        public static let summaries: [EnvironmentSummary] = [
            summary(
                id: billingID, name: "billing", folder: "billing", status: .down, ready: 0,
                total: 1),
            summary(
                id: erpID, name: "erp-development", folder: "erp", status: .partiallyReady,
                ready: 1, total: 4),
            summary(
                id: reportingID, name: "reporting", folder: "reporting", status: .ready, ready: 1,
                total: 1),
        ]

        /// The services of erp-development: one ready, one stopping, one borrowed and failed,
        /// and one not created yet.
        static let erpServices: [EnvironmentService] = [
            EnvironmentService(
                alias: "postgres", provider: "official/postgresql", ownership: .dedicated,
                selector: "17", instanceRef: nil, instance: serviceInstance(instance(1)),
                notReady: nil),
            EnvironmentService(
                alias: "cache", provider: "official/valkey", ownership: .dedicated,
                selector: "stable", instanceRef: nil, instance: serviceInstance(instance(3)),
                notReady: ServiceNotReady(reason: .stopping, detail: nil)),
            EnvironmentService(
                alias: "queue", provider: "official/nats", ownership: .shared, selector: nil,
                instanceRef: "queue", instance: serviceInstance(instance(5)),
                notReady: ServiceNotReady(reason: .failed, detail: "crash loop")),
            EnvironmentService(
                alias: "search", provider: "official/clickhouse", ownership: .dedicated,
                selector: "stable", instanceRef: nil, instance: nil,
                notReady: ServiceNotReady(reason: .notCreated, detail: nil)),
        ]

        /// The detail of an environment, with the manifest state `state` (`nil` for a detail
        /// read without `checkManifest`).
        public static func detail(
            _ id: UUID, manifest state: ManifestState? = .inSync, problems: UInt64? = nil
        ) -> EnvironmentDetail? {
            guard let summary = summaries.first(where: { $0.id == id }) else { return nil }
            let services: [EnvironmentService]
            var detached: [DetachedMember] = []
            switch id {
            case erpID:
                services = erpServices
                detached = [
                    DetachedMember(
                        alias: "legacy",
                        instance: InstanceRef(instanceId: instance(7).id, name: instance(7).name),
                        ownership: .dedicated)
                ]
            case billingID:
                services = [
                    EnvironmentService(
                        alias: "db", provider: "official/mysql", ownership: .dedicated,
                        selector: "8.4", instanceRef: nil, instance: serviceInstance(instance(4)),
                        notReady: ServiceNotReady(reason: .stopped, detail: nil))
                ]
            default:
                services = [
                    EnvironmentService(
                        alias: "db", provider: "official/postgresql", ownership: .shared,
                        selector: nil, instanceRef: "pg17-dev",
                        instance: serviceInstance(instance(1)), notReady: nil)
                ]
            }
            return EnvironmentDetail(
                summary: summary, services: services, detached: detached,
                manifestState: state, problems: state == .invalid ? (problems ?? 2) : problems)
        }

        // MARK: - Validation

        /// The findings of an invalid manifest: a cycle and a literal secret.
        public static let findings: [EnvFinding] = [
            EnvFinding(
                severity: .error, code: "dependency_cycle", location: "services.api.dependsOn",
                service: "api", message: "dependency cycle: api -> db -> api"),
            EnvFinding(
                severity: .error, code: "secret_literal",
                location: "services.db.secretRefs.password", service: "db",
                message: "secretRefs.password must be a local: reference, not a value"),
        ]

        /// What `env.validate` answers for a manifest: valid, or with ``findings``.
        public static func validation(
            path: String, name: String = "erp-development", valid: Bool = true
        ) -> EnvValidation {
            EnvValidation(
                file: path, valid: valid, apiVersion: "stackly/v1alpha1",
                supportedApiVersions: ["stackly/v1alpha1"], name: name,
                manifestSha256: String(repeating: "d", count: 64), services: [],
                findings: valid ? [] : findings, findingCount: valid ? 0 : UInt64(findings.count),
                omitted: 0)
        }

        // MARK: - Plans

        private static func error(
            _ code: String, _ category: ErrorCategory, _ message: String, hint: String? = nil
        ) -> ErrorBody {
            ErrorBody(
                code: code, category: category, message: message, retryable: false,
                resource: nil, fields: nil, hint: hint, details: nil, operationId: nil, rule: nil,
                items: nil)
        }

        private static func create(
            _ alias: String, provider: String, version: String, port: UInt16?,
            secret: (field: String, status: PlanSecretStatus)? = nil,
            trust: PlanTrust = .trusted, install: Bool = true, prefix: String = "erp-development"
        ) -> PlanItem {
            PlanItem(
                alias: alias, ownership: .dedicated, action: .create,
                steps: install
                    ? [.installRuntime, .createInstance, .startInstance]
                    : [.createInstance, .startInstance],
                instance: PlanInstance(id: nil, name: "\(prefix)-\(alias)"), provider: provider,
                trust: trust,
                runtime: PlanRuntime(
                    selector: "stable", version: version,
                    artifactId: "\(alias)-\(version)-macos-aarch64",
                    artifactSha256: String(repeating: "a", count: 64), install: install,
                    runtimeId: nil, downloadSize: install ? 104_857_600 : nil),
                endpoints: [
                    PlanEndpoint(
                        name: "client", host: "127.0.0.1", port: port.map { .fixed($0) } ?? .auto)
                ],
                secrets: secret.map {
                    [
                        PlanSecret(
                            field: $0.field,
                            reference: $0.status == .generated
                                ? nil : "local:erp/\(alias)/\($0.field)",
                            status: $0.status)
                    ]
                } ?? [],
                waitsFor: [], differences: [], checks: [])
        }

        /// The plan of the ERP manifest on a new Mac: six installs, six creates, six starts
        /// (spec 0044 AC-25).
        public static func freshPlan(path: String = manifest("erp")) -> EnvPlan {
            let items = [
                create(
                    "postgres", provider: "official/postgresql", version: "18.6", port: 5432,
                    secret: ("superuserPassword", .present)),
                create(
                    "mysql", provider: "official/mysql", version: "8.4.6", port: 3306,
                    secret: ("rootPassword", .unregistered)),
                create(
                    "nats", provider: "official/nats", version: "2.15.1", port: 4222,
                    secret: ("clientPassword", .present)),
                create(
                    "clickhouse", provider: "official/clickhouse", version: "25.8.3", port: 8123,
                    secret: ("defaultPassword", .present)),
                create(
                    "valkey", provider: "official/valkey", version: "8.1.4", port: 6379,
                    secret: ("password", .generated)),
                create("traefik", provider: "official/traefik", version: "3.7.2", port: nil),
            ]
            return EnvPlan(
                validation: validation(path: path), registration: .new, environment: nil,
                registeredLocation: nil, applicable: true,
                planDigest: String(repeating: "e", count: 64), items: items,
                order: items.map(\.alias), blockers: [], detached: [], needsSecrets: 1)
        }

        /// The plan of erp-development on this Mac: keep, start, a create whose provider needs
        /// approval, a difference, and two blockers.
        public static func registeredPlan() -> EnvPlan {
            let keep = PlanItem(
                alias: "postgres", ownership: .dedicated, action: .keep, steps: [],
                instance: PlanInstance(id: instance(1).id, name: "pg17-dev"),
                provider: "official/postgresql", trust: nil, runtime: nil,
                endpoints: [PlanEndpoint(name: "client", host: "127.0.0.1", port: .fixed(5432))],
                secrets: [], waitsFor: [],
                differences: [PlanDifference(field: "runtime", manifest: "18", instance: "17.6")],
                checks: [])
            let start = PlanItem(
                alias: "queue", ownership: .shared, action: .start, steps: [.startInstance],
                instance: PlanInstance(id: instance(5).id, name: "queue"),
                provider: "official/nats", trust: nil, runtime: nil, endpoints: [], secrets: [],
                waitsFor: ["postgres"], differences: [], checks: [])
            let created = create(
                "search", provider: "acme/search", version: "1.4.0", port: 5432,
                secret: ("adminPassword", .missing), trust: .approvalRequired, install: true)
            let items = [keep, start, created]
            return EnvPlan(
                validation: validation(path: manifest("erp")), registration: .registered,
                environment: EnvironmentRef(id: erpID, name: "erp-development", revision: 2),
                registeredLocation: manifest("erp"), applicable: false, planDigest: nil,
                items: items, order: ["postgres", "queue", "search"],
                blockers: [
                    PlanBlocker(
                        service: "search",
                        error: error(
                            "supervisor.endpoint_conflict", .conflict,
                            "port 5432 is already used by instance pg17-dev",
                            hint: "choose another port in the manifest")),
                    PlanBlocker(
                        service: "search",
                        error: error(
                            "provider.approval_required", .conflict,
                            "provider acme/search is not approved",
                            hint: "review the provider in the Catalog")),
                ],
                detached: [
                    DetachedMember(
                        alias: "legacy",
                        instance: InstanceRef(instanceId: instance(7).id, name: instance(7).name),
                        ownership: .dedicated)
                ], needsSecrets: 1)
        }

        /// The plan of a manifest that is not valid: no items, the findings.
        public static func invalidPlan(path: String = manifest("broken")) -> EnvPlan {
            EnvPlan(
                validation: validation(path: path, name: "broken", valid: false),
                registration: nil, environment: nil, registeredLocation: nil, applicable: false,
                planDigest: nil, items: [], order: [], blockers: [], detached: [],
                needsSecrets: 0)
        }

        /// The plan of a file whose name is registered at another location.
        public static func nameTakenPlan(path: String = manifest("erp-copy")) -> EnvPlan {
            let fresh = freshPlan(path: path)
            return EnvPlan(
                validation: fresh.validation, registration: .nameTaken,
                environment: EnvironmentRef(id: erpID, name: "erp-development", revision: 2),
                registeredLocation: manifest("erp"), applicable: false, planDigest: nil,
                items: fresh.items, order: fresh.order,
                blockers: [
                    PlanBlocker(
                        service: nil,
                        error: error(
                            "env.name_taken", .conflict,
                            "environment erp-development is registered at ~/Projects/erp/stackly.yaml"
                        ))
                ], detached: [], needsSecrets: fresh.needsSecrets)
        }

        /// The replies of the `environment-partially-ready` scenario: `env.list`; the fixture
        /// link answers `env.get` from ``detail(_:manifest:problems:)``.
        static var replies: [String: FixtureReply] {
            ["env.list": .reply(.envList(EnvironmentList(items: summaries)))]
        }
    }

    nonisolated extension InstanceSummary {
        /// The same summary with `environments` and the ownership they give.
        func with(environments: [InstanceEnvironment]) -> InstanceSummary {
            InstanceSummary(
                id: id, name: name, displayName: displayName, revision: revision,
                software: software, provider: provider, runtimeId: runtimeId,
                runtimeVersion: runtimeVersion, lifecycle: lifecycle, desired: desired,
                readiness: readiness, health: health, statusSeq: statusSeq, process: process,
                lastFailure: lastFailure, restart: restart, restartTotal: restartTotal,
                blocker: blocker, endpoints: endpoints, leftoverProcesses: leftoverProcesses,
                logsUnavailable: logsUnavailable,
                ownership: environments.contains { $0.ownership == .dedicated }
                    ? .dedicated : .shared,
                initialized: initialized, capabilities: capabilities, lastExit: lastExit,
                autostart: autostart, environments: environments)
        }
    }
#endif
