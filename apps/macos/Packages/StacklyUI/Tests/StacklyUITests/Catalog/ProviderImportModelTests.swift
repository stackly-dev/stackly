import Foundation
import StacklyClient
import Testing

@testable import StacklyUI

/// The import flow and the approval decisions of a local provider (spec 0037, AC-15): every
/// state of ``ProviderImportModel``, the digest each mutation sends, and what each refusal
/// leaves on screen.
@MainActor
struct ProviderImportModelTests {
    /// A daemon that answers the preview and records every mutation.
    final class Daemon {
        var preview: Result<ImportPreview, ClientError>
        var outcome: MutationOutcome
        var reads: [Call] = []
        var sent: [(Call, String)] = []
        /// Whether a mutation waits for ``release()`` before it answers.
        var holdsSends = false
        private(set) var held: [CheckedContinuation<Void, Never>] = []

        /// Lets every held mutation answer.
        func release() {
            let all = held
            held = []
            for one in all { one.resume() }
        }

        init(preview: Result<ImportPreview, ClientError>, outcome: MutationOutcome) {
            self.preview = preview
            self.outcome = outcome
        }

        func call(_ call: Call) async throws(ClientError) -> Reply {
            reads.append(call)
            switch preview {
            case .success(let preview): return .providerPreviewImport(preview)
            case .failure(let error): throw error
            }
        }

        func send(_ call: Call, _ key: String) async -> MutationOutcome {
            sent.append((call, key))
            if holdsSends { await withCheckedContinuation { held.append($0) } }
            return outcome
        }
    }

    private static func rejection(_ code: String, message: String = "refused") -> ErrorBody {
        ErrorBody(
            code: code, category: .conflict, message: message, retryable: false, resource: nil,
            fields: nil, hint: nil, details: nil, operationId: nil, rule: nil, items: nil)
    }

    private func model(
        _ daemon: Daemon, path: String? = "/Users/me/providers/acme-demo",
        imported: @escaping @MainActor (ProviderDetail) async -> Void = { _ in }
    ) -> ProviderImportModel {
        ProviderImportModel(
            picker: FixedFolderPicker(path: path),
            call: { call throws(ClientError) in try await daemon.call(call) },
            send: { call, key in await daemon.send(call, key) },
            newKey: { "key-1" },
            imported: imported)
    }

    // MARK: - Import

    @Test func cancelInTheFolderPanelLeavesNoSheetAndReadsNothing() async throws {
        let daemon = Daemon(
            preview: .success(try ProviderFixture.preview("pending")),
            outcome: .notSent(ClientErrorCatalog.notConnected()))
        let model = model(daemon, path: nil)
        await model.begin()
        #expect(model.phase == .idle)
        #expect(model.isPresented == false)
        #expect(daemon.reads.isEmpty)
    }

    @Test(
        .evidence(
            covers: ["REQ-033", "AC-033"],
            expected:
                "Choosing a folder previews it with provider.preview_import and sends nothing that mutates"
        ))
    func aChosenFolderIsPreviewedWithoutAMutation() async throws {
        let preview = try ProviderFixture.preview("pending")
        let daemon = Daemon(
            preview: .success(preview), outcome: .notSent(ClientErrorCatalog.notConnected()))
        let model = model(daemon)
        await model.begin()
        #expect(model.phase == .preview(preview, path: "/Users/me/providers/acme-demo"))
        #expect(model.isPresented)
        #expect(
            daemon.reads == [
                .providerPreviewImport(PreviewImportParams(path: "/Users/me/providers/acme-demo"))
            ])
        #expect(daemon.sent.isEmpty)
    }

    @Test func aFolderThatCantBeReadShowsTheDaemonsReason() async throws {
        let body = Self.rejection(
            "provider.import_unreadable", message: "the provider folder can't be read")
        let daemon = Daemon(
            preview: .failure(.rejected(body)), outcome: .notSent(ClientErrorCatalog.notConnected())
        )
        let model = model(daemon)
        await model.begin()
        #expect(model.phase == .unreadable(body, path: "/Users/me/providers/acme-demo"))
        #expect(model.isPresented)
    }

    @Test(
        .evidence(
            covers: ["REQ-033"],
            expected: "Import sends provider.import with the packageSha256 the sheet showed"
        ))
    func importSendsTheDigestTheSheetShowed() async throws {
        let preview = try ProviderFixture.preview("pending")
        let detail = try ProviderFixture.detail("pending")
        let daemon = Daemon(
            preview: .success(preview), outcome: .completed(.providerImport(detail)))
        var importedDetail: ProviderDetail?
        let model = model(daemon, imported: { importedDetail = $0 })
        await model.begin()
        await model.importPackage()
        let digest = try #require(preview.packageSha256)
        #expect(daemon.sent.count == 1)
        #expect(
            daemon.sent.first?.0
                == .providerImport(
                    ImportProviderParams(
                        path: "/Users/me/providers/acme-demo", expectedPackageSha256: digest)))
        #expect(daemon.sent.first?.1 == "key-1")
        #expect(model.phase == .idle)
        #expect(importedDetail == detail)
    }

    @Test func aRefusedImportKeepsThePreviewAndSaysWhy() async throws {
        let preview = try ProviderFixture.preview("pending")
        let body = Self.rejection("provider.import_changed")
        let daemon = Daemon(preview: .success(preview), outcome: .rejected(body))
        let model = model(daemon)
        await model.begin()
        await model.importPackage()
        #expect(
            model.phase == .failed(preview, path: "/Users/me/providers/acme-demo", error: body))
        #expect(model.preview == preview)
        await model.previewAgain()
        #expect(model.phase == .preview(preview, path: "/Users/me/providers/acme-demo"))
    }

    @Test func aPackageThatCantBeImportedSendsNothing() async throws {
        let preview = try ProviderFixture.preview("invalid")
        let daemon = Daemon(
            preview: .success(preview), outcome: .notSent(ClientErrorCatalog.notConnected()))
        let model = model(daemon)
        await model.begin()
        await model.importPackage()
        #expect(daemon.sent.isEmpty)
        #expect(ProviderImportModel.canImport(preview) == false)
        #expect(ImportProviderSheet.disabledReason(preview) == "This package can’t be imported.")
    }

    @Test func theSameBytesNeedNoImportUnlessTheStoredCopyIsDamaged() throws {
        let same = try ProviderFixture.preview("pending") { json in
            json["existing"] = [
                "providerRevision": "1.0.0",
                "packageSha256": json["packageSha256"] ?? "",
                "approval": "approved", "samePackage": true, "storedCopyValid": true,
            ]
        }
        let damaged = try ProviderFixture.preview("pending") { json in
            json["existing"] = [
                "providerRevision": "1.0.0",
                "packageSha256": json["packageSha256"] ?? "",
                "approval": "approved", "samePackage": true, "storedCopyValid": false,
            ]
        }
        #expect(ProviderImportModel.canImport(same) == false)
        #expect(ImportProviderSheet.disabledReason(same) == ProviderTrustCopy.samePackage)
        #expect(ImportPreviewBody.notes(of: same).contains(ProviderTrustCopy.samePackage))
        #expect(ProviderImportModel.canImport(damaged))
        #expect(ImportPreviewBody.notes(of: damaged).contains(ProviderTrustCopy.damagedCopy))
    }

    @Test func aReplacingPackageSaysWhatItReplacesAndWhatChanged() throws {
        let preview = try ProviderFixture.preview("replaces")
        #expect(
            ImportPreviewBody.notes(of: preview) == [
                ProviderTrustCopy.replaces(revision: "1.0.0"),
                "Differs from the version you decided on: revision, files it downloads, download hosts.",
            ])
    }

    @Test func aQuarantinedFolderGetsItsNote() throws {
        let preview = try ProviderFixture.preview("pending")
        #expect(preview.quarantined == true)
        #expect(ImportPreviewBody.notes(of: preview).first == ProviderTrustCopy.quarantineNote)
    }

    @Test func theProblemsOfAnInvalidPackageListErrorsThenImportIssuesAndCutAtTwenty() throws {
        let preview = try ProviderFixture.preview("invalid")
        let problems = ImportPreviewBody.problems(of: preview)
        #expect(problems.map(\.place) == ["provider.yaml metadata.id"])
        let many = (0..<25).map {
            ImportPreviewBody.Problem(place: "versions.json artifacts[\($0)]", message: "bad")
        }
        let items = ImportPreviewBody.items(many, omitted: 2)
        #expect(items.count == 21)
        #expect(items.last?.name == "and 7 more")
    }

    // MARK: - Approve, revoke, remove

    private func decisions(_ daemon: Daemon, refreshed: @escaping @MainActor () -> Void = {})
        -> ProviderDecisionModel
    {
        ProviderDecisionModel(
            send: { call, key in await daemon.send(call, key) }, refresh: { refreshed() },
            newKey: { "key-2" })
    }

    @Test(
        .evidence(
            covers: ["REQ-032"],
            expected:
                "Approve sends provider.approve with the packageSha256 of the detail the sheet shows"
        ))
    func approveSendsTheDigestOfTheDetailTheSheetShows() async throws {
        let detail = try ProviderFixture.detail("pending")
        let approved = try ProviderFixture.detail("approved")
        let daemon = Daemon(
            preview: .failure(.notSent(ClientErrorCatalog.notConnected())),
            outcome: .completed(.providerApprove(approved)))
        var refreshes = 0
        let model = decisions(daemon, refreshed: { refreshes += 1 })
        model.beginApprove(detail)
        #expect(model.approval?.detail == detail)
        await model.confirmApprove(feedback: nil)
        let digest = try #require(detail.local?.packageSha256)
        #expect(
            daemon.sent.first?.0
                == .providerApprove(
                    DecideProviderParams(provider: "acme/demo", expectedPackageSha256: digest)))
        #expect(model.approval == nil)
        #expect(refreshes == 1)
    }

    @Test func aPackageThatChangedClosesTheSheetAndLoadsTheDetailAgain() async throws {
        let daemon = Daemon(
            preview: .failure(.notSent(ClientErrorCatalog.notConnected())),
            outcome: .rejected(Self.rejection("provider.import_changed")))
        var refreshes = 0
        let model = decisions(daemon, refreshed: { refreshes += 1 })
        model.beginApprove(try ProviderFixture.detail("pending"))
        await model.confirmApprove(feedback: nil)
        #expect(model.approval == nil)
        #expect(refreshes == 1)
    }

    @Test func anotherRefusalStaysInTheSheet() async throws {
        let body = Self.rejection("provider.revoked")
        let daemon = Daemon(
            preview: .failure(.notSent(ClientErrorCatalog.notConnected())),
            outcome: .rejected(body))
        let model = decisions(daemon)
        model.beginApprove(try ProviderFixture.detail("pending"))
        await model.confirmApprove(feedback: nil)
        #expect(model.approval?.error == body)
        #expect(model.approval?.isWorking == false)
        model.cancelApprove()
        #expect(model.approval == nil)
    }

    @Test func revokeAndRemoveAskFirstWithTheirDestructiveCopy() async throws {
        let detail = try ProviderFixture.detail("approved")
        let daemon = Daemon(
            preview: .failure(.notSent(ClientErrorCatalog.notConnected())),
            outcome: .completed(.providerRevoke(detail)))
        let model = decisions(daemon)
        model.requestRevoke(detail, feedback: nil)
        let revoke = try #require(model.confirmation)
        #expect(revoke.title == "Revoke approval for “Acme Demo”?")
        #expect(revoke.role == .destructive)
        #expect(revoke.copyProblems.isEmpty)
        #expect(daemon.sent.isEmpty)
        model.requestRemove(detail, feedback: nil)
        let remove = try #require(model.confirmation)
        #expect(remove.title == "Remove “Acme Demo” from the catalog?")
        #expect(remove.actionTitle == "Remove")
        await model.revoke(detail, feedback: nil)
        await model.remove(detail, feedback: nil)
        #expect(
            daemon.sent.map(\.0) == [
                .providerRevoke(ProviderTargetParams(provider: "acme/demo")),
                .providerRemove(ProviderTargetParams(provider: "acme/demo")),
            ])
    }

    @Test func revokeShowsWhileTheEntryHasAnApproval() throws {
        #expect(LocalProviderSection.canRevoke(try ProviderFixture.detail("approved")))
        #expect(LocalProviderSection.canRevoke(try ProviderFixture.changedDetail()))
        #expect(LocalProviderSection.canRevoke(try ProviderFixture.detail("pending")) == false)
        #expect(LocalProviderSection.canRevoke(try ProviderFixture.detail("revoked")) == false)
    }

    // MARK: - Permissions

    @Test func thePermissionGroupsKeepTheirOrderAndLeaveEmptyOnesOut() throws {
        let permissions = try #require(try ProviderFixture.detail("pending").local?.permissions)
        #expect(
            ProviderPermissionsList.groups(of: permissions).map(\.rawValue) == [
                "Runs", "Downloads", "Writes to", "Receives secrets", "Listens on", "Opens",
                "Features",
            ])
        let downloads = permissions.downloads ?? []
        #expect(
            ProviderPermissionsList.hosts(of: downloads).map(\.host) == [
                "downloads.example.test", "mirror.example.test",
            ])
        let empty = ProviderPermissions(
            capabilities: [], managedFilesystem: ["data"], executables: [], secrets: [],
            endpoints: [], downloads: [], openWith: [])
        #expect(ProviderPermissionsList.groups(of: empty) == [.writes])
    }

    // MARK: - Cancel while a request runs (spec 0037 review)

    // covers: spec 0037 review, Minor (late results after Cancel)
    @Test func cancelIsOffWhileTheImportRunsAndItsAnswerLands() async throws {
        let preview = try ProviderFixture.preview("pending")
        let detail = try ProviderFixture.detail("pending")
        let daemon = Daemon(
            preview: .success(preview), outcome: .completed(.providerImport(detail)))
        daemon.holdsSends = true
        var importedDetail: ProviderDetail?
        let model = model(daemon, imported: { importedDetail = $0 })
        await model.begin()
        let running = Task { await model.importPackage() }
        await settle("the held import") { daemon.held.count == 1 }

        #expect(model.canCancel == false)
        model.cancel()
        #expect(model.phase == .importing(preview, path: "/Users/me/providers/acme-demo"))

        daemon.release()
        await running.value
        #expect(model.phase == .idle)
        #expect(importedDetail == detail)
        #expect(model.canCancel)
    }

    // covers: spec 0037 review, Minor (late results after Cancel)
    @Test func cancelIsOffWhileTheApprovalRuns() async throws {
        let detail = try ProviderFixture.detail("pending")
        let daemon = Daemon(
            preview: .failure(.notSent(ClientErrorCatalog.notConnected())),
            outcome: .completed(.providerApprove(try ProviderFixture.detail("approved"))))
        daemon.holdsSends = true
        let model = decisions(daemon)
        model.beginApprove(detail)
        let running = Task { await model.confirmApprove(feedback: nil) }
        await settle("the held approval") { daemon.held.count == 1 }

        model.cancelApprove()
        #expect(model.approval?.isWorking == true)

        daemon.release()
        await running.value
        #expect(model.approval == nil)
    }

    // covers: spec 0037 review, Minor (late results after Cancel)
    @Test func aRefusalNeverWritesToTheSheetOfAnotherProvider() async throws {
        let detail = try ProviderFixture.detail("pending")
        let other = try ProviderFixture.detail("revoked")
        let daemon = Daemon(
            preview: .failure(.notSent(ClientErrorCatalog.notConnected())),
            outcome: .rejected(Self.rejection("provider.not_approved")))
        daemon.holdsSends = true
        let model = decisions(daemon)
        model.beginApprove(detail)
        let running = Task { await model.confirmApprove(feedback: nil) }
        await settle("the held approval") { daemon.held.count == 1 }

        model.approval = ProviderDecisionModel.Approval(detail: other)
        daemon.release()
        await running.value
        #expect(model.approval == ProviderDecisionModel.Approval(detail: other))
    }
}
