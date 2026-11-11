import Foundation
import StacklyClient
import Testing

/// The import, approval, revocation, and removal methods of protocol 4.0
/// from the shared fixtures (spec 0037, *API surface*): requests, replies,
/// the `catalog.changed` event, the local detail, and values a newer
/// daemon sends, which read as `.unrecognized`.
struct ProviderImportContractTests {
    static func preview(_ file: String) throws -> ImportPreview {
        try StacklyJSON.decoder().decode(
            ImportPreview.self, from: Fixture.data("provider/\(file)"))
    }

    static func detail(_ file: String) throws -> ProviderDetail {
        try StacklyJSON.decoder().decode(
            ProviderDetail.self, from: Fixture.data("provider/\(file)"))
    }

    @Test func theImportRequestCarriesThePathAndTheDigestItShowed() throws {
        let request = try Fixture.ipc(Request.self, "request-provider-import.json")
        guard case .providerImport(let params) = request.call else {
            Issue.record("expected provider.import")
            return
        }
        #expect(params.path == "/Users/dev/providers/acme-demo")
        #expect(params.expectedPackageSha256 == String(repeating: "f", count: 64))
        #expect(request.idempotencyKey != nil)
    }

    @Test func thePreviewReplyIsThePendingFixture() throws {
        guard
            case .response(let response) = try Fixture.ipc(
                ServerFrame.self, "frame-response-provider-preview-import.json"),
            case .providerPreviewImport(let reply) = response.reply
        else {
            Issue.record("expected a provider.preview_import reply")
            return
        }
        #expect(reply == (try Self.preview("import-preview-pending.json")))
        #expect(reply.verdict.importable)
        #expect(reply.publisher == "acme")
        #expect(reply.permissions?.executables?.first?.uses == [.start, .versionCheck])
    }

    @Test func aReplacingPreviewNamesTheExistingImportAndTheChanges() throws {
        let reply = try Self.preview("import-preview-replaces.json")
        #expect(reply.existing?.approval == .approved)
        #expect(reply.existing?.samePackage == false)
        #expect(reply.changes == [.revision, .downloads, .downloadHosts])
    }

    @Test func aLocalDetailIsLocalTrustWithItsApproval() throws {
        let pending = try Self.detail("detail-local-pending.json")
        #expect(pending.summary.trust == .local)
        #expect(pending.summary.approval == .pending)
        #expect(pending.summary.stateReason == .approvalRequired)
        let revoked = try Self.detail("detail-local-revoked.json")
        #expect(revoked.revocation?.by == .user)
        #expect(revoked.local?.decision?.decision == .revoked)
    }

    @Test func theCatalogChangedEventNamesTheProviderAndTheChange() throws {
        guard
            case .event(let event) = try Fixture.ipc(
                ServerFrame.self, "frame-event-catalog-changed.json"),
            case .catalogChanged(let changed) = event.kind
        else {
            Issue.record("expected a catalog.changed event")
            return
        }
        #expect(changed.provider == "acme/demo")
        #expect(changed.change == .imported)
    }

    @Test func valuesANewerDaemonSendsReadUnrecognized() throws {
        let reply = try Self.preview("import-preview-unknown-values.json")
        #expect(reply.verdict.reason == .unrecognized)
        #expect(reply.changes == [.unrecognized])
        #expect(reply.existing?.approval == .unrecognized)
        #expect(reply.revocation?.by == .unrecognized)
        #expect(reply.permissions?.executables?.first?.uses?.last == .unrecognized)
    }

    @Test func theApprovalStatementIsOneLine() throws {
        let text = try #require(
            String(data: try Fixture.data("provider/approval-statement.txt"), encoding: .utf8))
        #expect(text.hasPrefix("This provider runs native programs on your Mac"))
        #expect(text.filter { $0 == "\n" }.count == 1)
    }
}
