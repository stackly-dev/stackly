import StacklyClient
import SwiftUI
import Testing

@testable import StacklyUI

/// The Catalog page in every variant (spec 0068 AC-8, spec 0017 AC-10, AC-11): the chosen
/// provider in each of its shapes (available, unsupported, planned, invalid), and the
/// states of the page (no results, empty, failed, unsupported daemon, stale catalog).
///
/// The page reads the fixtures of `protocols/fixtures/catalog/`, the files the Rust golden
/// tests of `stackly provider` read.
extension SnapshotTests {
    struct CatalogSnapshotTests {
        /// What a snapshot changes about the catalog the fixtures describe.
        private struct Setup {
            var selection = ""
            var filter = CatalogFilter()
            var failure: ClientError?
            var items: [ProviderSummary]?
            var outdated = false
            var invalidCount: UInt64?
        }

        @MainActor
        private func page(_ setup: Setup = Setup()) async throws -> some View {
            let script = try CatalogScript.fixture()
            if var list = script.list {
                let info = list.catalog
                list = ListProvidersReply(
                    catalog: CatalogInfo(
                        revision: info.revision, publishedAt: info.publishedAt,
                        loadedAt: info.loadedAt, contentSha256: info.contentSha256,
                        source: info.source, trust: info.trust, host: info.host,
                        outdated: setup.outdated,
                        invalidCount: setup.invalidCount ?? info.invalidCount),
                    items: setup.items ?? list.items)
                script.list = list
            }
            script.failure = setup.failure
            let store = script.store()
            await store.store.resnapshot()
            script.failure = nil
            for provider in store.providers ?? [] {
                await store.detail.load(provider.provider, revision: provider.revision)
            }
            return CatalogPage(
                catalog: store, waiting: nil, hasConnection: true, reconnect: {},
                openDiagnostics: {}, selection: setup.selection, filter: setup.filter
            )
            .actionHost()
        }

        private func unknownMethod() -> ClientError {
            .rejected(
                ErrorBody(
                    code: "request.unknown_method", category: .unsupported,
                    message: "unknown method", retryable: false, resource: nil, fields: nil,
                    hint: nil, details: nil, operationId: nil, rule: nil, items: nil))
        }

        private func unavailable() -> ClientError {
            .rejected(
                ErrorBody(
                    code: "catalog.unavailable", category: .unavailable,
                    message: "the provider catalog is not available", retryable: false,
                    resource: nil, fields: nil,
                    hint: "run stacklyd from Stackly.app or set STACKLY_CATALOG_DIR",
                    details: nil, operationId: nil, rule: nil, items: nil))
        }

        @Test(
            .evidence(
                covers: ["REQ-002", "NFR-012"],
                expected:
                    "The Catalog page with an available provider chosen renders in every variant"
            ))
        func catalogWithAnAvailableProvider() async throws {
            try await assertSnapshot(
                page(Setup(selection: "official/nats")), named: "catalog-available")
        }

        @Test(
            .evidence(
                covers: ["REQ-002", "NFR-012"],
                expected:
                    "The Catalog page with an unsupported provider chosen renders in every variant"
            ))
        func catalogWithAnUnsupportedProvider() async throws {
            try await assertSnapshot(
                page(Setup(selection: "official/intelonly")), named: "catalog-unsupported")
        }

        @Test(
            .evidence(
                covers: ["REQ-002", "NFR-012"],
                expected:
                    "The Catalog page with a blocked provider chosen renders in every variant"
            ))
        func catalogWithABlockedProvider() async throws {
            try await assertSnapshot(
                page(Setup(selection: "official/blockedhost")), named: "catalog-blocked")
        }

        @Test(
            .evidence(
                covers: ["REQ-002", "NFR-012"],
                expected:
                    "The Catalog page with a planned provider chosen renders in every variant"
            ))
        func catalogWithAPlannedProvider() async throws {
            try await assertSnapshot(
                page(Setup(selection: "official/postgresql")), named: "catalog-planned")
        }

        @Test(
            .evidence(
                covers: ["REQ-002", "NFR-012"],
                expected:
                    "The Catalog page with an invalid provider chosen renders in every variant"
            ))
        func catalogWithAnInvalidProvider() async throws {
            try await assertSnapshot(
                page(Setup(selection: "official/broken")), named: "catalog-invalid")
        }

        @Test(
            .evidence(
                covers: ["REQ-002", "NFR-012"],
                expected: "The Catalog page for a search with no results renders in every variant"
            ))
        func catalogWithNoResults() async throws {
            try await assertSnapshot(
                page(Setup(filter: CatalogFilter(query: "zzz"))), named: "catalog-no-results")
        }

        @Test(
            .evidence(
                covers: ["REQ-002", "NFR-012"],
                expected: "The Catalog page of a catalog with no providers renders in every variant"
            ))
        func catalogWithNoProviders() async throws {
            try await assertSnapshot(
                page(Setup(items: [], invalidCount: 0)), named: "catalog-empty")
        }

        @Test(
            .evidence(
                covers: ["REQ-002", "NFR-012"],
                expected: "The Catalog page of a stale catalog renders in every variant"
            ))
        func catalogThatIsOutdated() async throws {
            try await assertSnapshot(
                page(Setup(selection: "official/nats", outdated: true, invalidCount: 0)),
                named: "catalog-outdated")
        }

        @Test(
            .evidence(
                covers: ["REQ-002", "NFR-012"],
                expected: "The Catalog page that failed to load renders in every variant"
            ))
        func catalogThatFailedToLoad() async throws {
            try await assertSnapshot(
                page(Setup(failure: unavailable())), named: "catalog-failed")
        }

        @Test(
            .evidence(
                covers: ["REQ-002", "NFR-012"],
                expected:
                    "The Catalog page of a daemon without the catalog renders in every variant"
            ))
        func catalogOfADaemonWithoutTheMethod() async throws {
            try await assertSnapshot(
                page(Setup(failure: unknownMethod())), named: "catalog-daemon-unsupported")
        }
    }
}
