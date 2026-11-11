import StacklyClient
import SwiftUI
import Testing

@testable import StacklyUI

/// Imported providers in every variant (spec 0037 AC-15, spec 0068): the Import Provider
/// sheet for a package that can be imported, one that can't, and one that replaces an
/// earlier import; the Catalog page with a local entry in each approval state; and the
/// approve sheet.
///
/// Every value comes from the fixtures of `protocols/fixtures/provider/`, the files the
/// Rust tests and the CLI golden read.
extension SnapshotTests {
    struct CatalogLocalSnapshotTests {
        private static var isPrimed = false
        private static let sheetSize = SnapshotSize.fitting(width: 520, narrowWidth: 520)

        /// Renders a sheet once and keeps nothing: the first sheet a process draws looks
        /// different from every later one (the reason the New Instance snapshots give).
        @MainActor
        private static func prime() async throws {
            guard !isPrimed else { return }
            isPrimed = true
            _ = try await render(
                sheet(try ProviderFixture.preview("pending")), variant: .light, size: sheetSize)
        }

        @MainActor
        private static func sheet(_ preview: ImportPreview) async -> some View {
            let model = ProviderImportModel(
                picker: FixedFolderPicker(path: nil),
                call: { _ throws(ClientError) in .providerPreviewImport(preview) },
                send: { _, _ in .notSent(ClientErrorCatalog.notConnected()) },
                imported: { _ in })
            await model.read("/Users/me/providers/acme-demo")
            return ImportProviderSheet(model: model)
        }

        @MainActor
        private func page(_ detail: ProviderDetail) async throws -> some View {
            let script = try CatalogScript.fixture()
            let list = try #require(script.list)
            let info = list.catalog
            script.list = ListProvidersReply(
                catalog: CatalogInfo(
                    revision: info.revision, publishedAt: info.publishedAt,
                    loadedAt: info.loadedAt, contentSha256: info.contentSha256,
                    source: info.source, trust: info.trust, host: info.host,
                    outdated: false, invalidCount: 0, localCount: 1),
                items: (list.items ?? []).filter { $0.state != .invalid } + [detail.summary],
                revocations: [])
            script.details[detail.summary.provider] = detail
            let store = script.store()
            await store.store.resnapshot()
            for provider in store.providers ?? [] {
                await store.detail.load(provider.provider, revision: provider.revision)
            }
            let runtimes = RuntimeStore(
                call: { call throws(ClientError) in
                    guard case .runtimeList = call else {
                        throw .rejected(ClientErrorCatalog.protocolViolation())
                    }
                    return .runtimeList(RuntimeListPage(items: [], next: nil, installing: []))
                }, now: { Date(timeIntervalSince1970: 100) }, sleep: { _ in })
            await runtimes.store.resnapshot()
            return CatalogPage(
                catalog: store, runtimes: runtimes, waiting: nil, hasConnection: true,
                reconnect: {}, openDiagnostics: {}, selection: detail.summary.provider,
                picker: FixedFolderPicker(path: nil)
            )
            .actionHost()
        }

        // MARK: - Import sheet

        @Test(
            .evidence(
                covers: ["REQ-033", "NFR-012"],
                expected:
                    "The Import Provider sheet of an importable package shows its rows, permissions, quarantine note, and footnote in every variant"
            ))
        @MainActor
        func theImportSheetOfAnImportablePackage() async throws {
            try await Self.prime()
            try await assertSnapshot(
                Self.sheet(try ProviderFixture.preview("pending")), named: "catalog-import-preview",
                size: Self.sheetSize)
        }

        @Test(
            .evidence(
                covers: ["REQ-033", "NFR-012"],
                expected:
                    "The Import Provider sheet of a package that can't be imported says why in every variant"
            ))
        @MainActor
        func theImportSheetOfAnInvalidPackage() async throws {
            try await Self.prime()
            try await assertSnapshot(
                Self.sheet(try ProviderFixture.preview("invalid")), named: "catalog-import-invalid",
                size: Self.sheetSize)
        }

        @Test(
            .evidence(
                covers: ["REQ-032", "NFR-012"],
                expected:
                    "The Import Provider sheet of a replacing package names the revision it replaces and the changes in every variant"
            ))
        @MainActor
        func theImportSheetOfAReplacingPackage() async throws {
            try await Self.prime()
            try await assertSnapshot(
                Self.sheet(try ProviderFixture.preview("replaces")),
                named: "catalog-import-replaces",
                size: Self.sheetSize)
        }

        @Test(
            .evidence(
                covers: ["REQ-033", "NFR-012"],
                expected:
                    "The Import Provider sheet of a folder that can't be read says why, offers another folder, and shows no spinner in every variant"
            ))
        @MainActor
        func theImportSheetOfAFolderThatCantBeRead() async throws {
            try await Self.prime()
            let model = ProviderImportModel(
                picker: FixedFolderPicker(path: nil),
                call: { _ throws(ClientError) in
                    throw .rejected(
                        ErrorBody(
                            code: "provider.import_unreadable", category: .invalidInput,
                            message: "the folder can't be read", retryable: false,
                            resource: nil, fields: nil, hint: nil, details: nil,
                            operationId: nil, rule: nil, items: nil))
                },
                send: { _, _ in .notSent(ClientErrorCatalog.notConnected()) },
                imported: { _ in })
            await model.read("/Users/me/providers/acme-demo")
            try await assertSnapshot(
                ImportProviderSheet(model: model), named: "catalog-import-unreadable",
                size: Self.sheetSize)
        }

        // MARK: - Detail panel

        @Test(
            .evidence(
                covers: ["REQ-032", "AC-033", "NFR-012"],
                expected:
                    "A local provider waiting for approval shows its banner, rows, permissions, and buttons in every variant"
            ))
        @MainActor
        func aPendingLocalProvider() async throws {
            try await assertSnapshot(
                page(try ProviderFixture.detail("pending")), named: "catalog-local-pending")
        }

        @Test(
            .evidence(
                covers: ["REQ-032", "AC-032", "NFR-012"],
                expected:
                    "A local provider that changed since its approval names the changes in every variant"
            ))
        @MainActor
        func aLocalProviderThatChanged() async throws {
            try await assertSnapshot(
                page(try ProviderFixture.changedDetail()), named: "catalog-local-changed")
        }

        @Test(
            .evidence(
                covers: ["REQ-032", "NFR-012"],
                expected:
                    "An approved local provider shows no banner and offers Revoke Approval… in every variant"
            ))
        @MainActor
        func anApprovedLocalProvider() async throws {
            try await assertSnapshot(
                page(try ProviderFixture.detail("approved")), named: "catalog-local-approved")
        }

        @Test(
            .evidence(
                covers: ["REQ-032", "AC-072", "NFR-012"],
                expected:
                    "A local provider the person revoked offers Approve Again… in every variant"
            ))
        @MainActor
        func aRevokedLocalProvider() async throws {
            try await assertSnapshot(
                page(try ProviderFixture.detail("revoked")), named: "catalog-local-revoked")
        }

        @Test(
            .evidence(
                covers: ["REQ-032", "AC-072", "NFR-012"],
                expected:
                    "A local provider the installed catalog revokes says the catalog revision and date in every variant"
            ))
        @MainActor
        func aLocalProviderTheCatalogRevokes() async throws {
            try await assertSnapshot(
                page(try ProviderFixture.catalogRevokedDetail()),
                named: "catalog-local-catalog-revoked")
        }

        // MARK: - Approve sheet

        @Test(
            .evidence(
                covers: ["REQ-032", "BR-020", "AC-061", "NFR-012"],
                expected:
                    "The approve sheet shows the approval statement, the permissions, and the footnote with Cancel as the default in every variant"
            ))
        @MainActor
        func theApproveSheet() async throws {
            try await Self.prime()
            let model = ProviderDecisionModel(
                send: { _, _ in .notSent(ClientErrorCatalog.notConnected()) }, refresh: {})
            model.beginApprove(try ProviderFixture.changedDetail())
            try await assertSnapshot(
                ApproveProviderSheet(model: model), named: "catalog-approve-sheet",
                size: Self.sheetSize)
        }
    }
}
