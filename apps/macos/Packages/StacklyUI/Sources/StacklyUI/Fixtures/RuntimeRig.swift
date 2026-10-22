#if DEBUG
    import Foundation
    import StacklyClient
    import SwiftUI

    /// The stores the Runtimes page and the Catalog's Get column need, loaded from the
    /// runtime fixtures and with answers the caller controls (spec 0018: previews, snapshots,
    /// and page tests).
    @MainActor
    final class RuntimeRig {
        /// What the daemon answers.
        final class Script {
            var page: RuntimeListPage
            var operations: [OperationSummary]
            var providers: [ProviderSummary]
            var listFailure: ClientError?
            var getFailure: ClientError?
            var sent: [Call] = []

            init(
                page: RuntimeListPage, operations: [OperationSummary], providers: [ProviderSummary]
            ) {
                self.page = page
                self.operations = operations
                self.providers = providers
            }
        }

        let script: Script
        let runtimes: RuntimeStore
        let catalog: CatalogStore
        let operations: OperationCenter

        /// Builds the rig; nothing is loaded until ``load()``.
        init(
            page: RuntimeListPage = FixtureRuntime.page,
            operations ops: [OperationSummary] = FixtureScenario.operationCases,
            providers: [ProviderSummary] = PageRig.providers
        ) {
            let script = Script(page: page, operations: ops, providers: providers)
            self.script = script
            let now: () -> Date = { FixtureClock.now }
            runtimes = RuntimeStore(
                call: { call throws(ClientError) in
                    script.sent.append(call)
                    switch call {
                    case .runtimeList:
                        if let failure = script.listFailure { throw failure }
                        return .runtimeList(script.page)
                    case .runtimeGet(let params):
                        if let failure = script.getFailure { throw failure }
                        guard
                            let found = (script.page.items ?? []).first(where: {
                                $0.id == params.runtimeId
                            })
                        else {
                            throw .rejected(
                                .fixtureNotFound(
                                    .runtime, id: params.runtimeId.uuidString.lowercased()))
                        }
                        return .runtimeGet(FixtureRuntime.detail(of: found))
                    default:
                        throw .rejected(PageRig.unknownMethod)
                    }
                },
                send: { _, _ in .notSent(ClientErrorCatalog.notConnected()) },
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
                fetchPage: { _ throws(ClientError) in Page(items: script.operations, next: nil) },
                fetchOne: { id throws(ClientError) in script.operations.first { $0.id == id } },
                now: now, sleep: { _ in })
        }

        /// Loads every store once, as the first snapshot round of the shell does.
        func load() async {
            await catalog.store.resnapshot()
            await operations.store.resnapshot()
            await runtimes.store.resnapshot()
        }

        /// The Runtimes page over these stores, inside the action host of a window.
        func runtimesPage(
            waiting: FailureContent? = nil, hasConnection: Bool = true, selection: String = "",
            showsInspector: Bool = true
        ) -> some View {
            RuntimesPage(
                runtimes: runtimes, catalog: catalog, operations: operations, waiting: waiting,
                hasConnection: hasConnection, reconnect: {}, openDiagnostics: {},
                selection: selection, showsInspector: showsInspector
            )
            .actionHost()
        }
    }
#endif
