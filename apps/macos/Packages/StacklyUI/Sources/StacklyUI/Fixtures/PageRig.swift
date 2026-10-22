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

            init(
                instances: [InstanceSummary], operations: [OperationSummary],
                providers: [ProviderSummary]
            ) {
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
        let operations: OperationCenter
        let capacity: CapacityModel
        let actions: InstanceActionModel
        /// What the actions sent, in order.
        private(set) var sent: [Call] = []

        /// Builds the rig; nothing is loaded until ``load()``.
        init(
            instances items: [InstanceSummary] = FixtureScenario.instanceCases,
            operations ops: [OperationSummary] = FixtureScenario.operationCases,
            providers: [ProviderSummary] = PageRig.providers,
            capacity report: CapacityReport? = nil
        ) {
            let script = Script(instances: items, operations: ops, providers: providers)
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
                        ListProvidersReply(catalog: .fixture, items: script.providers))
                }, now: now, sleep: { _ in })
            operations = OperationCenter(
                fetchPage: { _ throws(ClientError) in
                    if let failure = script.operationFailure { throw failure }
                    return Page(items: script.operations, next: nil)
                },
                fetchOne: { id throws(ClientError) in script.operations.first { $0.id == id } },
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
            actions = InstanceActionModel(
                send: { _, _ in .notSent(ClientErrorCatalog.notConnected()) },
                updates: { _ in AsyncStream { $0.finish() } },
                readOperation: { _ in nil },
                now: now, announce: { _ in })
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
            await capacity.refresh()
        }

        /// Loads only the stores a page of the Instances needs.
        func loadInstances() async {
            await instances.store.resnapshot()
            await catalog.store.resnapshot()
        }

        /// The Instances page over these stores, inside the action host of a window.
        func instancesPage(
            waiting: FailureContent? = nil, hasConnection: Bool = true, selection: String = "",
            showsInspector: Bool = true, filter: InstanceFilter = .all, query: String = "",
            sortOrder: InstanceSortComparator = .default
        ) -> some View {
            InstancesPage(
                instances: instances, catalog: catalog, actions: actions, operations: operations,
                waiting: waiting, hasConnection: hasConnection, reconnect: {}, openDiagnostics: {},
                selection: selection, showsInspector: showsInspector, filter: filter, query: query,
                sortOrder: sortOrder
            )
            .actionHost()
        }

        /// The Overview over these stores, inside the action host of a window.
        func overviewPage(
            isConnected: Bool = true, waiting: FailureContent? = nil, hasConnection: Bool = true
        ) -> some View {
            OverviewPage(
                instances: instances, catalog: catalog, operations: operations,
                capacity: capacity, actions: actions, isConnected: isConnected,
                waiting: waiting, hasConnection: hasConnection, reconnect: {},
                openDiagnostics: {}
            )
            .actionHost()
        }
    }

#endif
