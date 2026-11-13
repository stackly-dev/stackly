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

        /// A reason a service is not ready, with no Up detail.
        private static func notReady(_ reason: NotReadyReason, detail: String?) -> ServiceNotReady {
            ServiceNotReady(
                reason: reason, detail: detail, phase: nil, waitingOn: [], references: [],
                dependency: nil, dependencyReason: nil, stage: nil, code: nil, operationId: nil)
        }

        private static func serviceInstance(_ summary: InstanceSummary) -> ServiceInstance {
            ServiceInstance(
                id: summary.id, name: summary.name, runtimeVersion: summary.runtimeVersion,
                lifecycle: summary.lifecycle, readiness: summary.readiness,
                health: summary.health, endpoints: summary.endpoints)
        }

        // MARK: - Summaries and details

        /// The summary of an environment.
        public static func summary(
            id: UUID, name: String, folder: String?, status: EnvironmentStatus, ready: UInt64,
            total: UInt64, revision: UInt64 = 2
        ) -> EnvironmentSummary {
            EnvironmentSummary(
                id: id, name: name, revision: revision, manifestLocation: folder.map(manifest),
                manifestSha256: String(repeating: "d", count: 64), lockSha256: nil, status: status,
                readyCount: ready, serviceCount: total, updatedAt: FixtureClock.minutesAgo(20),
                activeOperation: nil)
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
                notReady: notReady(.stopping, detail: nil)),
            EnvironmentService(
                alias: "queue", provider: "official/nats", ownership: .shared, selector: nil,
                instanceRef: "queue", instance: serviceInstance(instance(5)),
                notReady: notReady(.failed, detail: "crash loop")),
            EnvironmentService(
                alias: "search", provider: "official/clickhouse", ownership: .dedicated,
                selector: "stable", instanceRef: nil, instance: nil,
                notReady: notReady(.notCreated, detail: nil)),
        ]

        // MARK: - Lock and local override (spec 0045)

        /// The lock file of a detail in `state`: erp-development locks three services for
        /// Apple silicon.
        public static func lockInfo(_ state: LockState, targets: [String] = ["macos-aarch64"])
            -> EnvLockInfo
        {
            let readable = state == .inSync || state == .stale
            return EnvLockInfo(
                file: "stackly.lock", state: state, targets: readable ? targets : [],
                services: readable ? 3 : 0)
        }

        /// The port override of erp-development: `postgres` on 5434.
        public static let portOverride = PlanOverride(
            service: "postgres", field: "endpoints.client.port", value: "5434", state: .applied)
        /// The storage override of erp-development: `cache` data on the external disk.
        public static let storageOverride = PlanOverride(
            service: "cache", field: "storage.dataLocation", value: "/Volumes/Fast/Stackly",
            state: .applied)

        /// The local override file of a detail: `state` `nil` means there is no file.
        public static func localInfo(
            _ state: LocalState?, overrides: [PlanOverride] = [], problems: UInt64 = 0
        ) -> EnvLocalInfo {
            EnvLocalInfo(
                file: ".stackly.local.yaml", state: state, overrides: overrides,
                problems: problems)
        }

        /// What `env.get` with `checkManifest` answers for the lock and the local file of an
        /// environment: erp-development is locked with two overrides, reporting's lock is out
        /// of date, and billing has neither.
        static func files(_ id: UUID) -> (EnvLockInfo, EnvLocalInfo) {
            switch id {
            case erpID:
                (
                    lockInfo(.inSync),
                    localInfo(.present, overrides: [portOverride, storageOverride])
                )
            case reportingID: (lockInfo(.stale), localInfo(nil))
            default: (lockInfo(.absent), localInfo(nil))
            }
        }

        /// The detail of an environment, with the manifest state `state` (`nil` for a detail
        /// read without `checkManifest`). The lock and local file come only with a manifest
        /// that is `in_sync` or `changed`, as the daemon sends them; `files` replaces them.
        public static func detail(
            _ id: UUID, manifest state: ManifestState? = .inSync, problems: UInt64? = nil,
            files override: (EnvLockInfo?, EnvLocalInfo?)? = nil
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
                        notReady: notReady(.stopped, detail: nil))
                ]
            default:
                services = [
                    EnvironmentService(
                        alias: "db", provider: "official/postgresql", ownership: .shared,
                        selector: nil, instanceRef: "pg17-dev",
                        instance: serviceInstance(instance(1)), notReady: nil)
                ]
            }
            let sendsFiles = state == .inSync || state == .changed
            let (lock, local): (EnvLockInfo?, EnvLocalInfo?) =
                override ?? (sendsFiles ? files(id) : (nil, nil))
            return EnvironmentDetail(
                summary: summary, services: services, detached: detached,
                manifestState: state, problems: state == .invalid ? (problems ?? 2) : problems,
                lock: lock, local: local, lastUp: nil)
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
            trust: PlanTrust = .trusted, install: Bool = true, prefix: String = "erp-development",
            source: RuntimeSource? = nil
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
                    runtimeId: nil, downloadSize: install ? 104_857_600 : nil, source: source),
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
                waitsFor: [], differences: [], checks: [], providerRevision: nil, storage: nil)
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
                order: items.map(\.alias), blockers: [], detached: [], needsSecrets: 1,
                lock: nil, local: nil)
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
                checks: [], providerRevision: nil, storage: nil)
            let start = PlanItem(
                alias: "queue", ownership: .shared, action: .start, steps: [.startInstance],
                instance: PlanInstance(id: instance(5).id, name: "queue"),
                provider: "official/nats", trust: nil, runtime: nil, endpoints: [], secrets: [],
                waitsFor: ["postgres"], differences: [], checks: [], providerRevision: nil,
                storage: nil)
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
                ], needsSecrets: 1, lock: nil, local: nil)
        }

        /// The plan of erp-development with its lock and local file (spec 0045): a keep whose
        /// runtime differs from the lock, two creates from the lock, one with the port
        /// override, a new service from the catalog that the lock does not hold yet, and a
        /// warning of the local file.
        public static func lockedPlan() -> EnvPlan {
            let keep = PlanItem(
                alias: "postgres", ownership: .dedicated, action: .keep, steps: [],
                instance: PlanInstance(id: instance(1).id, name: "pg17-dev"),
                provider: "official/postgresql", trust: nil, runtime: nil,
                endpoints: [PlanEndpoint(name: "client", host: "127.0.0.1", port: .fixed(5432))],
                secrets: [], waitsFor: [],
                differences: [PlanDifference(field: "lock", manifest: "17.7", instance: "17.6")],
                checks: [], providerRevision: nil, storage: nil)
            let mysql = create(
                "mysql", provider: "official/mysql", version: "8.4.6", port: 3307,
                secret: ("rootPassword", .present), install: false, source: .lock)
            let nats = create(
                "nats", provider: "official/nats", version: "2.15.1", port: 4222,
                secret: ("clientPassword", .present), source: .lock)
            let search = create(
                "search", provider: "official/clickhouse", version: "25.8.3", port: 8123,
                secret: ("defaultPassword", .present), source: .catalog)
            let items = [keep, mysql, nats, search]
            return EnvPlan(
                validation: validation(path: manifest("erp")), registration: .registered,
                environment: EnvironmentRef(id: erpID, name: "erp-development", revision: 2),
                registeredLocation: manifest("erp"), applicable: true,
                planDigest: String(repeating: "e", count: 64), items: items,
                order: items.map(\.alias), blockers: [], detached: [], needsSecrets: 0,
                lock: PlanLock(
                    file: "stackly.lock", state: .stale, sha256: String(repeating: "b", count: 64),
                    locked: false, targets: ["macos-aarch64", "macos-x86_64"],
                    stale: [LockStaleItem(service: "search", reason: .notLocked)]),
                local: PlanLocal(
                    file: ".stackly.local.yaml", state: .present,
                    sha256: String(repeating: "c", count: 64),
                    overrides: [
                        PlanOverride(
                            service: "mysql", field: "endpoints.client.port", value: "3307",
                            state: .applied),
                        PlanOverride(
                            service: "postgres", field: "endpoints.client.port", value: "5434",
                            state: .compared),
                        PlanOverride(
                            service: "legacy", field: "endpoints.client.port", value: "6000",
                            state: .ignored),
                    ],
                    findings: [
                        EnvFinding(
                            severity: .warning, code: "override_unknown_service",
                            location: "services.legacy", service: "legacy",
                            message:
                                "service legacy is not in the manifest, its override is ignored")
                    ]))
        }

        /// The same plan with `locked` true and a local file with an error: the stale entry
        /// and the local file are blockers, and no override shows as applied.
        public static func lockedBlockedPlan() -> EnvPlan {
            let plan = lockedPlan()
            // A local file with an error is not applied, so mysql keeps the manifest port.
            let mysql = create(
                "mysql", provider: "official/mysql", version: "8.4.6", port: 3306,
                secret: ("rootPassword", .present), install: false, source: .lock)
            let items = (plan.items ?? []).compactMap { item -> PlanItem? in
                switch item.alias {
                case "search": nil
                case "mysql": mysql
                default: item
                }
            }
            return EnvPlan(
                validation: plan.validation, registration: plan.registration,
                environment: plan.environment, registeredLocation: plan.registeredLocation,
                applicable: false, planDigest: nil, items: items, order: items.map(\.alias),
                blockers: [
                    PlanBlocker(
                        service: "search",
                        error: error(
                            "env.lock_stale", .conflict,
                            "service search is not in the lock",
                            hint: "run stackly env lock to update the lock")),
                    PlanBlocker(
                        service: nil,
                        error: error(
                            "env.local_invalid", .invalidInput,
                            ".stackly.local.yaml has 1 problem",
                            hint: "fix the local override file")),
                ], detached: [], needsSecrets: 0,
                lock: PlanLock(
                    file: "stackly.lock", state: .stale, sha256: plan.lock?.sha256,
                    locked: true, targets: plan.lock?.targets, stale: plan.lock?.stale),
                local: PlanLocal(
                    file: ".stackly.local.yaml", state: .invalid, sha256: plan.local?.sha256,
                    overrides: (plan.local?.overrides ?? []).map {
                        PlanOverride(
                            service: $0.service, field: $0.field, value: $0.value, state: .ignored)
                    },
                    findings: [
                        EnvFinding(
                            severity: .error, code: "unknown_endpoint",
                            location: "services.mysql.endpoints.admin", service: "mysql",
                            message: "mysql has no IP endpoint named admin")
                    ]))
        }

        /// The plan of a manifest that is not valid next to a lock that reads `in_sync` and a
        /// local file with an applied override (spec 0045 K23): the sheet shows only Problems.
        public static func invalidLockedPlan() -> EnvPlan {
            let invalid = invalidPlan(path: manifest("erp"))
            let locked = lockedPlan()
            return EnvPlan(
                validation: invalid.validation, registration: nil, environment: nil,
                registeredLocation: nil, applicable: false, planDigest: nil, items: [],
                order: [], blockers: [], detached: [], needsSecrets: 0,
                lock: PlanLock(
                    file: "stackly.lock", state: .inSync, sha256: locked.lock?.sha256,
                    locked: false, targets: locked.lock?.targets, stale: []),
                local: locked.local)
        }

        /// What `env.lock` answers for erp-development: three services written.
        public static func lockResult(written: Bool = true) -> EnvLockResult {
            let services = [
                LockedService(
                    alias: "postgres", provider: "official/postgresql", selector: "17",
                    version: "17.7", artifactId: "postgresql-17.7-macos-aarch64",
                    change: written ? .added : .kept, previousVersion: nil),
                LockedService(
                    alias: "cache", provider: "official/valkey", selector: "stable",
                    version: "8.1.4", artifactId: "valkey-8.1.4-macos-aarch64",
                    change: written ? .added : .kept, previousVersion: nil),
                LockedService(
                    alias: "search", provider: "official/clickhouse", selector: "stable",
                    version: "25.8.3", artifactId: "clickhouse-25.8.3-macos-aarch64",
                    change: written ? .added : .kept, previousVersion: nil),
            ]
            return EnvLockResult(
                validation: validation(path: manifest("erp")),
                lockFile: "\(FixtureDetail.home)/Projects/erp/stackly.lock", written: written,
                changed: written, sha256: String(repeating: "b", count: 64),
                targets: ["macos-aarch64"], services: services, blockers: [])
        }

        /// What `env.lock` answers when a provider needs approval: nothing is written.
        public static func lockBlocked() -> EnvLockResult {
            EnvLockResult(
                validation: validation(path: manifest("erp")),
                lockFile: "\(FixtureDetail.home)/Projects/erp/stackly.lock", written: false,
                changed: false, sha256: nil, targets: [], services: [],
                blockers: [
                    PlanBlocker(
                        service: "search",
                        error: error(
                            "provider.approval_required", .conflict,
                            "provider acme/search is not approved",
                            hint: "review the provider in the Catalog"))
                ])
        }

        /// What `env.export` answers for erp-development: the three variables of `postgres`,
        /// or none before Up.
        public static func export(empty: Bool = false) -> EnvExport {
            let variables =
                empty
                ? []
                : [
                    ExportVariable(
                        name: "STACKLY_POSTGRES_CLIENT_HOST", value: "127.0.0.1",
                        service: "postgres", endpoint: "client", kind: .host, mayChange: false),
                    ExportVariable(
                        name: "STACKLY_POSTGRES_CLIENT_PORT", value: "5432", service: "postgres",
                        endpoint: "client", kind: .port, mayChange: false),
                    ExportVariable(
                        name: "STACKLY_POSTGRES_CLIENT_ADDRESS", value: "127.0.0.1:5432",
                        service: "postgres", endpoint: "client", kind: .address,
                        mayChange: false),
                ]
            return EnvExport(
                environment: EnvironmentRef(id: erpID, name: "erp-development", revision: 2),
                format: .dotenv,
                text: variables.map { "\($0.name)=\($0.value)\n" }.joined(),
                variables: variables,
                skipped: [ExportSkipped(service: "search", endpoint: nil, reason: .notCreated)])
        }

        /// More than five environments, for the Show All of the Overview (spec 0044 AC-23).
        public static let manySummaries: [EnvironmentSummary] =
            summaries
            + (4...7).map { number in
                summary(
                    id: id(UInt8(number)), name: "project-\(number)", folder: "project-\(number)",
                    status: .down, ready: 0, total: 2)
            }

        /// The plan of a manifest that is not valid: no items, the findings.
        public static func invalidPlan(path: String = manifest("broken")) -> EnvPlan {
            EnvPlan(
                validation: validation(path: path, name: "broken", valid: false),
                registration: nil, environment: nil, registeredLocation: nil, applicable: false,
                planDigest: nil, items: [], order: [], blockers: [], detached: [],
                needsSecrets: 0, lock: nil, local: nil)
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
                ], detached: [], needsSecrets: fresh.needsSecrets, lock: nil, local: nil)
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

#if DEBUG
    /// A file panel that answers a fixed path, or cancels with `nil`.
    struct FixtureFilePicker: FilePicking {
        /// The path it answers.
        let path: String?

        func chooseFile(message: String, extensions: [String], startingAt: String?) async
            -> String?
        {
            path
        }
    }
#endif
