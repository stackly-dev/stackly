#if DEBUG
    import Foundation
    import StacklyClient
    import SwiftUI

    /// The stores a page of the Overview or the Instances needs, loaded from fixture data and
    /// with fetchers the caller controls (spec 0022: previews, snapshots, and page tests).
    @MainActor
    final class PageRig {
        /// What the daemon answers.
        final class Script {
            var instances: [InstanceSummary]
            var operations: [OperationSummary]
            var providers: [ProviderSummary]
            var instanceFailure: ClientError?
            var operationFailure: ClientError?
            var oneFailure: ClientError?
            /// What `runtime.list` answers.
            var runtimePage: RuntimeListPage
            var runtimeFailure: ClientError?
            /// What `instance.get_detail` answers instead of the fixture detail, by instance.
            var detailOverrides: [UUID: InstanceDetail] = [:]
            /// The failure `instance.get_detail` answers, or `nil`.
            var detailFailure: ClientError?
            /// What `instance.get_data_usage` answers instead of the fixture size, by instance;
            /// a `nil` value is "no fresh result".
            var usageOverrides: [UUID: DataUsage?] = [:]
            /// The failure `instance.get_data_usage` answers, or `nil`.
            var usageFailure: ClientError?
            /// Whether `instance.get_detail` never answers, so the page stays loading.
            var detailHangs = false
            /// The failure `instance.prepare_terminal` answers, or `nil`.
            var prepareFailure: ClientError?
            /// The `(instance, action)` pairs `instance.prepare_terminal` was asked for.
            var prepared: [(instance: UUID, action: String)] = []
            /// The ended operations `operation.list { resource }` answers besides
            /// ``operations``, such as those of ``FixtureFailureBanner`` (spec 0033).
            var instanceOperations: [OperationSummary] = []
            /// The failure `operation.list { resource }` answers, or `nil`.
            var instanceOperationsFailure: ClientError?
            /// What `audit.list { instanceId }` of the Activity tab answers (spec 0035).
            var activity = FixtureActivity.Case.rows
            /// What `env.list` answers (spec 0044).
            var environments: [EnvironmentSummary] = []
            /// The failure `env.list` answers, or `nil`.
            var environmentFailure: ClientError?
            /// The manifest state `env.get` with `checkManifest` answers.
            var manifestState: ManifestState = .inSync
            /// What `env.get` answers instead of the fixture detail, by environment.
            var environmentDetails: [UUID: EnvironmentDetail] = [:]
            /// The failure `env.get` answers, or `nil`.
            var environmentDetailFailure: ClientError?
            /// What `env.validate` answers, or `nil` for a valid manifest.
            var validation: EnvValidation?
            /// What `env.plan` answers, or `nil` for the fixture plan of the source.
            var plan: EnvPlan?
            /// The failure `env.plan` answers, or `nil`.
            var planFailure: ClientError?
            /// What `env.import` and `env.remove` end with, or `nil` for a completed one.
            var environmentOutcome: MutationOutcome?
            /// The environment mutations sent, in order.
            var environmentSent: [Call] = []
            /// What `env.lock` answers, or `nil` for the fixture result (spec 0045).
            var lockResult: EnvLockResult?
            /// What `env.export` answers, or `nil` for the fixture export.
            var export: EnvExport?
            /// The failure `env.lock` and `env.export` answer, or `nil`.
            var lockFailure: ClientError?
            /// The `env.lock` and `env.export` calls sent, in order.
            var lockCalls: [Call] = []

            /// What the environment reads answer.
            func environmentReply(_ call: Call) throws(ClientError) -> Reply {
                switch call {
                case .envList:
                    if let environmentFailure { throw environmentFailure }
                    return .envList(EnvironmentList(items: environments))
                case .envGet(let params):
                    if let environmentDetailFailure { throw environmentDetailFailure }
                    if let override = environmentDetails[params.id] { return .envGet(override) }
                    guard
                        let detail = FixtureEnvironments.detail(
                            params.id,
                            manifest: params.checkManifest == true ? manifestState : nil)
                    else {
                        throw .rejected(
                            .fixtureNotFound(.environment, id: params.id.uuidString.lowercased()))
                    }
                    return .envGet(detail)
                case .envValidate(let params):
                    return .envValidate(
                        validation ?? FixtureEnvironments.validation(path: params.path))
                case .envPlan(let params):
                    if let planFailure { throw planFailure }
                    if let plan { return .envPlan(plan) }
                    switch params.source {
                    case .file(let path): return .envPlan(FixtureEnvironments.freshPlan(path: path))
                    case .registered: return .envPlan(FixtureEnvironments.registeredPlan())
                    }
                case .envLock:
                    lockCalls.append(call)
                    if let lockFailure { throw lockFailure }
                    return .envLock(lockResult ?? FixtureEnvironments.lockResult())
                case .envExport:
                    lockCalls.append(call)
                    if let lockFailure { throw lockFailure }
                    return .envExport(export ?? FixtureEnvironments.export())
                default:
                    throw .rejected(PageRig.unknownMethod)
                }
            }

            /// What an environment mutation ends with.
            func environmentMutation(_ call: Call) -> MutationOutcome {
                environmentSent.append(call)
                if let environmentOutcome { return environmentOutcome }
                switch call {
                case .envImport(let params):
                    let id =
                        params.relinkEnvironmentId ?? FixtureEnvironments.id(9)
                    let detail =
                        FixtureEnvironments.detail(id)
                        ?? EnvironmentDetail(
                            summary: FixtureEnvironments.summary(
                                id: id, name: "erp-development", folder: "erp", status: .down,
                                ready: 0, total: 6, revision: 1),
                            services: [], detached: [], manifestState: nil, problems: nil,
                            lock: nil, local: nil)
                    return .completed(.envImport(detail))
                case .envRemove(let params):
                    let name = environments.first { $0.id == params.id }?.name ?? "environment"
                    return .completed(.envRemove(RemovedEnvironment(id: params.id, name: name)))
                default:
                    return .rejected(PageRig.unknownMethod)
                }
            }

            /// What `audit.list { instanceId }` answers: the rows of ``activity``.
            func activityPage(of id: UUID, before: String?) async throws(ClientError) -> AuditPage {
                try await FixtureActivity.read(activity)(id, before)
            }

            /// What `operation.list { resource: instance }` answers: the newest operations of
            /// one instance, at most `limit`.
            func operations(of id: UUID, limit: UInt32) throws(ClientError) -> [OperationSummary] {
                if let instanceOperationsFailure { throw instanceOperationsFailure }
                let key = id.uuidString.lowercased()
                let own = (operations + instanceOperations).filter {
                    $0.resource?.kind == .instance && $0.resource?.id == key
                }
                return Array(RelatedOperation.newestFirst(own).prefix(Int(limit)))
            }

            /// The script path `instance.prepare_terminal` answers for `action`.
            func prepareTerminal(_ id: UUID, _ action: String) throws(ClientError) -> String {
                prepared.append((id, action))
                if let prepareFailure { throw prepareFailure }
                return
                    "\(FixtureDetail.home)/Library/Application Support/Stackly/instances/temp/open-\(action).command"
            }

            func detail(of id: UUID) async throws(ClientError) -> InstanceDetail {
                while detailHangs, Task.isCancelled == false {
                    try? await Task.sleep(for: .seconds(3_600))
                }
                if let detailFailure { throw detailFailure }
                if let override = detailOverrides[id] { return override }
                guard let found = instances.first(where: { $0.id == id }) else {
                    throw .rejected(.fixtureNotFound(.instance, id: id.uuidString.lowercased()))
                }
                return FixtureDetail.detail(of: found)
            }

            func usage(of id: UUID) throws(ClientError) -> DataUsage? {
                if let usageFailure { throw usageFailure }
                if let override = usageOverrides[id] { return override }
                return instances.first(where: { $0.id == id }).map(FixtureDetail.usage(of:))
            }

            init(
                instances: [InstanceSummary], operations: [OperationSummary],
                providers: [ProviderSummary], runtimePage: RuntimeListPage
            ) {
                self.runtimePage = runtimePage
                self.instances = instances
                self.operations = operations
                self.providers = providers
            }
        }

        /// The catalog of the providers the fixture instances run.
        static let providers: [ProviderSummary] = [
            .fixture(
                provider: "official/postgresql", displayName: "PostgreSQL", mark: "Pg", tile: .blue),
            .fixture(
                provider: "official/mysql", displayName: "MySQL", versions: ["8.4.6"], mark: "My",
                tile: .orange),
            .fixture(
                provider: "official/nats", displayName: "NATS", category: .messaging,
                versions: ["2.11.9"], mark: "Na", tile: .green),
            .fixture(
                provider: "official/valkey", displayName: "Valkey", category: .cache,
                versions: ["8.1.4"], mark: "Vk", tile: .red),
            .fixture(
                provider: "official/clickhouse", displayName: "ClickHouse", category: .analytics,
                versions: ["25.8.3"], mark: "Ch", tile: .yellow),
        ]

        let script: Script
        let instances: InstanceStore
        let catalog: CatalogStore
        /// The runtime store, or `nil` when the rig was built without runtimes: the Overview
        /// then draws no Runtimes card.
        let runtimes: RuntimeStore?
        let operations: OperationCenter
        /// The environments (spec 0044).
        let environments: EnvironmentStore
        let capacity: CapacityModel
        let actions: InstanceActionModel
        /// What the detail page and the inspector read: the fixture detail of each instance,
        /// or what the script says.
        let detail: InstanceDetailService
        /// The applications and the opener of the Open In menus.
        let openIn: FixtureOpenIn
        /// What the actions sent, in order.
        private(set) var sent: [Call] = []

        /// Builds the rig; nothing is loaded until ``load()``.
        init(
            instances items: [InstanceSummary] = FixtureScenario.instanceCases,
            operations ops: [OperationSummary] = FixtureScenario.operationCases,
            providers: [ProviderSummary] = PageRig.providers,
            capacity report: CapacityReport? = nil,
            environments summaries: [EnvironmentSummary] = [],
            runtimes runtimePage: RuntimeListPage? = nil,
            openIn: FixtureOpenIn = FixtureOpenIn()
        ) {
            self.openIn = openIn
            let script = Script(
                instances: items, operations: ops, providers: providers,
                runtimePage: runtimePage ?? RuntimeListPage(items: [], next: nil, installing: []))
            script.environments = summaries
            self.script = script
            let now: () -> Date = { FixtureClock.now }
            instances = InstanceStore(
                fetchPage: { _ throws(ClientError) in
                    if let failure = script.instanceFailure { throw failure }
                    return Page(items: script.instances, next: nil)
                },
                fetchOne: { id throws(ClientError) in
                    if let failure = script.oneFailure { throw failure }
                    return script.instances.first { $0.id == id }
                },
                now: now, sleep: { _ in })
            catalog = CatalogStore(
                call: { call throws(ClientError) in
                    guard case .catalogListProviders = call else {
                        throw .rejected(ClientErrorCatalog.protocolViolation())
                    }
                    return .catalogListProviders(
                        ListProvidersReply(
                            catalog: .fixture, items: script.providers, revocations: []))
                }, now: now, sleep: { _ in })
            runtimes =
                runtimePage == nil
                ? nil
                : RuntimeStore(
                    call: { call throws(ClientError) in
                        guard case .runtimeList = call else {
                            throw .rejected(PageRig.unknownMethod)
                        }
                        if let failure = script.runtimeFailure { throw failure }
                        return .runtimeList(script.runtimePage)
                    },
                    send: { _, _ in .notSent(ClientErrorCatalog.notConnected()) },
                    now: now, sleep: { _ in })
            operations = OperationCenter(
                fetchPage: { _ throws(ClientError) in
                    if let failure = script.operationFailure { throw failure }
                    return Page(items: script.operations, next: nil)
                },
                fetchOne: { id throws(ClientError) in script.operations.first { $0.id == id } },
                now: now, sleep: { _ in })
            environments = EnvironmentStore(
                call: { call throws(ClientError) in try script.environmentReply(call) },
                send: { call, _ in script.environmentMutation(call) },
                now: now, sleep: { _ in })
            capacity = CapacityModel(
                read: { call throws(ClientError) in
                    guard case .storageGetCapacity = call, let report else {
                        throw .rejected(PageRig.unknownMethod)
                    }
                    return .storageGetCapacity(report)
                },
                // The poll waits here until the page goes away: a fixture does not read again.
                sleep: { _ in try await Task.sleep(for: .seconds(3_600)) })
            detail = InstanceDetailService(
                detail: { id throws(ClientError) in try await script.detail(of: id) },
                usage: { id, _, _ throws(ClientError) in try await script.usage(of: id) },
                instance: { id throws(ClientError) in
                    await script.instances.first { $0.id == id }
                },
                secretEvents: { AsyncStream { $0.finish() } },
                secrets: SecretsClient { _, _ throws(ClientError) in
                    throw .notSent(ClientErrorCatalog.notConnected())
                },
                prepareTerminal: { id, action throws(ClientError) in
                    try await script.prepareTerminal(id, action)
                },
                operations: { id, limit throws(ClientError) in
                    try await script.operations(of: id, limit: limit)
                },
                activity: { id, before throws(ClientError) in
                    try await script.activityPage(of: id, before: before)
                })
            actions = InstanceActionModel(
                send: { _, _ in .notSent(ClientErrorCatalog.notConnected()) },
                updates: { _ in AsyncStream { $0.finish() } },
                readOperation: { _ in nil },
                now: now)
        }

        /// A failure the daemon answers when it cannot read its registry.
        static let failure = ErrorBody(
            code: "internal.unexpected", category: .internal,
            message: "the registry could not be read", retryable: false, resource: nil, fields: nil,
            hint: nil, details: nil, operationId: nil, rule: nil, items: nil)

        /// `request.unknown_method`, what an older daemon answers.
        static let unknownMethod = ErrorBody(
            code: "request.unknown_method", category: .unsupported, message: "unknown method",
            retryable: false, resource: nil, fields: nil, hint: nil, details: nil, operationId: nil,
            rule: nil, items: nil)

        /// Loads every store once, as the first snapshot round of the shell does, and reads the
        /// disk once.
        func load() async {
            await instances.store.resnapshot()
            await catalog.store.resnapshot()
            await operations.store.resnapshot()
            await runtimes?.store.resnapshot()
            await environments.store.resnapshot()
            await capacity.refresh()
        }

        /// Loads only the stores a page of the Instances needs.
        func loadInstances() async {
            await instances.store.resnapshot()
            await catalog.store.resnapshot()
        }

        /// The Instances page over these stores, inside the action host of a window. The Logs
        /// tab of a detail page answers from the `logs` fixture and the Metrics tab from the `metrics`
        /// fixture, and the Configuration tab from the `configuration` fixture.
        func instancesPage(
            waiting: FailureContent? = nil, hasConnection: Bool = true, opening: UUID? = nil,
            openingTab: InstanceTab = .summary, logs: FixtureLogs.Case = .follow,
            metrics: FixtureMetrics.Case = .liveMain,
            configuration: FixtureConfiguration.Case = .postgresql,
            selection: String = "", showsInspector: Bool = true, filter: InstanceFilter = .all,
            query: String = "", sortOrder: InstanceSortComparator = .default
        ) -> some View {
            InstancesPage(
                instances: instances, catalog: catalog, actions: actions, operations: operations,
                waiting: waiting, hasConnection: hasConnection, reconnect: {}, openDiagnostics: {},
                detail: detail, logs: FixtureLogs.service(logs),
                metrics: FixtureMetrics.service(
                    metrics, instance: FixtureID.uuid(kind: FixtureID.instance, number: 1)),
                config: FixtureConfiguration.service(configuration),
                openIn: openIn.ports,
                opening: opening,
                openingTab: openingTab, selection: selection,
                showsInspector: showsInspector, filter: filter, query: query,
                sortOrder: sortOrder
            )
            .actionHost()
        }

        /// What the More menu of an environment opened and revealed, in order.
        private(set) var openedFiles: [URL] = []
        /// What Copy Environment Variables wrote to the pasteboard, in order (spec 0045).
        private(set) var copiedTexts: [String] = []

        /// The Environments section over these stores, inside the action host of a window:
        /// the list, or the detail page of `selected` loaded with the script's manifest state.
        /// A `plan` source opens the plan sheet, loaded, over the page.
        func environmentsPage(
            selected: UUID? = nil, waiting: FailureContent? = nil, hasConnection: Bool = true,
            plan: EnvironmentPlanPresenter.Source? = nil, validate: Bool = false,
            lock: LockRefresh? = nil, picker: any FilePicking = FixtureFilePicker(path: nil)
        ) async -> some View {
            var detailModel: EnvironmentDetailModel?
            if let selected {
                let model = EnvironmentDetailModel(
                    id: selected, store: environments,
                    copyText: { [weak self] in self?.copiedTexts.append($0) })
                await model.load(checkManifest: true)
                if validate { _ = await model.validate() }
                if let lock { _ = await model.lockVersions(refresh: lock) }
                detailModel = model
            }
            let presenter = EnvironmentPlanPresenter(store: environments, picker: picker)
            if let plan { await presenter.read(plan) }
            let files = ManifestFilePorts(
                open: { [weak self] in self?.openedFiles.append($0) },
                reveal: { [weak self] in self?.openedFiles.append($0) })
            return EnvironmentsPage(
                environments: environments, instances: instances, catalog: catalog,
                actions: actions, selected: selected, waiting: waiting,
                hasConnection: hasConnection, reconnect: {}, openDiagnostics: {},
                detail: detail, openIn: openIn.ports, files: files, picker: picker,
                home: FixtureDetail.home, presenter: presenter, detailModel: detailModel
            )
            .actionHost()
        }

        /// The Overview over these stores, inside the action host of a window.
        func overviewPage(
            isConnected: Bool = true, waiting: FailureContent? = nil, hasConnection: Bool = true,
            derivation: OverviewDerivation? = nil
        ) -> some View {
            let page = OverviewPage(
                instances: instances, catalog: catalog, runtimes: runtimes,
                environments: environments, operations: operations, capacity: capacity,
                actions: actions, isConnected: isConnected,
                waiting: waiting, hasConnection: hasConnection, reconnect: {},
                openDiagnostics: {}, home: FixtureDetail.home
            )
            return (derivation.map { page.derivation($0) } ?? page).actionHost()
        }
    }

#endif
