import Darwin
import Foundation
import StacklyClient
import StacklyTestSupport
import Testing

@testable import StacklyUI

/// Import and approval from the GUI side against a real `stacklyd` (spec 0037 AC-4,
/// AC-5, AC-7, AC-9, AC-15): the live shell model imports a copy of the fake package
/// under the `acme` namespace through ``ProviderImportModel``, approves, revokes, and
/// removes it through ``ProviderDecisionModel``, and a second connection checks the
/// gate and makes a change the ``CatalogStore`` must follow through `catalog.changed`.
///
/// The daemon runs on a root seeded by `skeleton_seed` (the fake runtime) with the empty
/// catalog of `stackly-testkit` as `STACKLY_CATALOG_DIR`, since `provider.*` needs a
/// loaded catalog. Without the binaries the test ends as BLOCKED (with evidence).
@Suite(.tags(.daemon), .timeLimit(.minutes(3)))
@MainActor
struct LocalProviderEndToEndTests {
    private static let local = "acme/fake"

    /// The `versions.json` of the copy: one available artifact, which no step downloads.
    /// A fixture artifact belongs only to the `testing` namespace.
    private static let versions = """
        {
          "schema": "stackly.provider-versions/v1",
          "software": "fake",
          "artifacts": [
            {
              "id": "fake-0.1.0-macos-aarch64",
              "line": "0.1",
              "version": "0.1.0",
              "os": "macos",
              "arch": "aarch64",
              "status": "available",
              "minOs": "13.0",
              "format": "tar_gz",
              "urls": ["https://downloads.example.test/fake/fake-0.1.0-macos-aarch64.tar.gz"],
              "sha256": "1111111111111111111111111111111111111111111111111111111111111111",
              "size": 1048576,
              "signingPolicy": "keep_or_adhoc",
              "license": "MIT",
              "licenseUrl": "https://downloads.example.test/fake/LICENSE",
              "executables": ["stackly-fake-server"],
              "stable": true
            }
          ]
        }
        """

    /// A folder with the empty catalog of `stackly-testkit`.
    private static func emptyCatalog() throws -> String {
        let dir = "/private/tmp/stk-cat-\(UUID().uuidString.prefix(8).lowercased())"
        try FileManager.default.createDirectory(
            atPath: dir, withIntermediateDirectories: false)
        let source = SourceTree.url(of: "crates/testkit/src/empty_catalog.json")
        try FileManager.default.copyItem(
            at: source, to: URL(filePath: dir).appending(path: "catalog.json"))
        return dir
    }

    /// A copy of `providers/fake` as `acme/fake` in the database category, under `root`.
    private static func localPackage(in root: String) throws -> String {
        let dir = "\(root)/packages/acme/fake"
        try FileManager.default.createDirectory(
            atPath: "\(root)/packages/acme", withIntermediateDirectories: true)
        try FileManager.default.copyItem(
            at: SourceTree.url(of: "providers/fake"), to: URL(filePath: dir))
        try? FileManager.default.removeItem(atPath: "\(dir)/README.md")
        let manifest = "\(dir)/provider.yaml"
        let text = try String(contentsOfFile: manifest, encoding: .utf8)
            .replacingOccurrences(of: "id: testing/fake", with: "id: acme/fake")
            .replacingOccurrences(of: "category: testing", with: "category: database")
            .replacingOccurrences(
                of: "displayName: Fake Provider", with: "displayName: Acme Fake")
        try text.write(toFile: manifest, atomically: true, encoding: .utf8)
        try versions.write(toFile: "\(dir)/versions.json", atomically: true, encoding: .utf8)
        return dir
    }

    private static func params(_ rig: SkeletonRig, name: String) -> CreateInstanceParams {
        CreateInstanceParams(
            name: name, displayName: nil,
            provider: ProviderSelector(name: local, revision: nil),
            runtimeId: rig.seed.runtimeId,
            endpoints: [EndpointRequest(name: "http", port: .auto)],
            settings: JSONObject([:]), secrets: nil, startup: nil, duplicateOf: nil)
    }

    /// The create form of the local entry, or the daemon's refusal.
    private static func form(_ rig: SkeletonRig) async throws -> (CreateForm?, ErrorBody?) {
        do {
            let reply = try await rig.send(
                .instanceGetCreateForm(
                    GetCreateFormParams(
                        runtimeId: rig.seed.runtimeId, provider: local, revision: nil)))
            guard case .instanceGetCreateForm(let form) = reply else {
                throw RigFailure(description: "instance.get_create_form gave the wrong reply")
            }
            return (form, nil)
        } catch let error as ClientError {
            return (nil, error.body)
        }
    }

    /// The refusal of `instance.create` for the local entry, or `nil` when it was accepted.
    private static func createRefusal(_ rig: SkeletonRig, name: String) async throws
        -> ErrorBody?
    {
        do {
            _ = try await rig.send(
                .instanceCreate(params(rig, name: name)), key: SkeletonRig.newKey())
            return nil
        } catch let error as ClientError {
            return error.body
        }
    }

    private static func detail(_ key: String, of body: ErrorBody) -> String? {
        body.details?.first { $0.key == key }?.value
    }

    private static func summary(_ catalog: CatalogStore) -> ProviderSummary? {
        catalog.providers?.first { $0.provider == local }
    }

    /// Loads the detail of the local entry through the model the panel reads, as the page
    /// does: one load per detail epoch, so a `catalog.changed` snapshot that lands while the
    /// load is on its way makes one more load for the new epoch, and nothing else retries.
    private static func loadedDetail(_ catalog: CatalogStore) async throws -> ProviderDetail {
        guard let summary = Self.summary(catalog) else {
            throw RigFailure(description: "\(local) is not in the catalog")
        }
        while true {
            let epoch = catalog.detail.epoch
            await catalog.detail.load(summary.provider, revision: summary.revision)
            switch catalog.detail.phase(for: summary.provider, revision: summary.revision) {
            case .loaded(let detail):
                return detail
            case .failed(let failure):
                throw RigFailure(description: "the detail of \(local) failed: \(failure.message)")
            case .loading:
                guard catalog.detail.epoch != epoch else {
                    throw RigFailure(description: "the detail of \(local) did not load")
                }
            }
        }
    }

    @Test(
        .evidence(
            covers: ["REQ-032", "REQ-033", "AC-033", "AC-072"],
            expected:
                "against a real stacklyd the GUI models import a local package as pending, the gate refuses create, approve opens it and create runs, revoke closes it with provider.revoked by user while the old instance still starts, a stale digest closes the approve sheet, an approval by another client reaches the CatalogStore through catalog.changed, and remove takes the entry away"
        ))
    func theGUIImportsApprovesRevokesAndRemovesAProviderAgainstARealDaemon() async throws {
        let catalogDir = try Self.emptyCatalog()
        defer { try? FileManager.default.removeItem(atPath: catalogDir) }
        try await SkeletonRig.run(
            recordsEvidence: true, environment: ["STACKLY_CATALOG_DIR": catalogDir]
        ) { rig in
            let parts = ShellEndToEndTests.shell(for: rig.daemon)
            let model = parts.runtime.model
            do {
                try await Self.walk(rig: rig, model: model)
            } catch {
                await parts.shutDown()
                throw error
            }
            await parts.shutDown()
        }
    }

    private static func walk(rig: SkeletonRig, model: DaemonModel) async throws {
        try await ShellEndToEndTests.waitUntilRefreshed(model, "the shell to connect and refresh")
        let catalog = model.catalog
        let feedback = FeedbackCenter(sleep: { _ in try await Task.sleep(for: .seconds(3600)) })
        let package = try localPackage(in: rig.daemon.root)

        // Import: the folder panel, the preview, then provider.import.
        var imported: ProviderDetail?
        let importer = catalog.makeImportModel(
            picker: FixedFolderPicker(path: package), imported: { imported = $0 })
        await importer.begin()
        let preview = try #require(importer.preview, "phase \(importer.phase)")
        #expect(preview.verdict.importable)
        #expect(preview.provider == local)
        #expect(ProviderImportModel.canImport(preview))
        await importer.importPackage()
        #expect(importer.phase == .idle, "phase \(importer.phase)")
        let first = try #require(imported)
        #expect(first.summary.approval == .pending)
        #expect(first.summary.trust == .local)
        #expect(first.local?.packageSha256 == preview.packageSha256)
        try await waitFor("the pending entry in the store", seconds: 20) {
            summary(catalog)?.approval == .pending
        }
        #expect(
            CatalogAppearance.badge(of: try #require(summary(catalog))).label == "Needs Approval")

        // Pending: the gate refuses the form and the create, and nothing starts.
        let pendingRefusal = try #require(try await form(rig).1, "a pending form was built")
        #expect(pendingRefusal.code == "provider.approval_required")
        let pendingCreate = try #require(try await createRefusal(rig, name: "gated"))
        #expect(pendingCreate.code == "provider.approval_required")

        // A digest that is not the stored one: the sheet closes and the detail loads again.
        let decisions = catalog.makeDecisionModel()
        let pendingDetail = try await loadedDetail(catalog)
        let stale = try changedDigest(pendingDetail)
        decisions.beginApprove(stale)
        await decisions.confirmApprove(feedback: feedback)
        #expect(decisions.approval == nil)
        #expect(feedback.current?.code == "provider.import_changed")
        #expect(summary(catalog)?.approval == .pending)
        // A failure stays until the person closes it; the next success replaces nothing.
        feedback.dismiss()

        // Approve with the digest the panel shows: the gate opens and a create runs.
        decisions.beginApprove(try await loadedDetail(catalog))
        await decisions.confirmApprove(feedback: feedback)
        #expect(decisions.approval == nil, "error \(String(describing: decisions.approval?.error))")
        #expect(feedback.current?.message == ProviderTrustCopy.approved(displayName: "Acme Fake"))
        try await waitFor("the approved entry in the store", seconds: 20) {
            summary(catalog)?.approval == .approved
        }
        let approved = try await form(rig)
        let approvedForm = try #require(
            approved.0, "the approved form was refused: \(String(describing: approved.1))")
        #expect(approvedForm.provider.source == .localImport)
        let reply = try await rig.send(
            .instanceCreate(params(rig, name: "local-one")), key: SkeletonRig.newKey())
        guard case .instanceCreate(let action) = reply, let accepted = action.accepted else {
            throw RigFailure(description: "instance.create gave no operation")
        }
        try await rig.requireSucceeded(accepted.operation.id, "create local-one")
        let created = try #require(
            try await rig.listInstances().first { $0.name == "local-one" }?.id)
        rig.adopt(created)

        // Revoke from the panel: a new create is refused with the daemon's words, and the
        // instance that exists still starts.
        let approvedDetail = try await loadedDetail(catalog)
        #expect(LocalProviderSection.canRevoke(approvedDetail))
        decisions.requestRevoke(approvedDetail, feedback: feedback)
        #expect(decisions.confirmation?.title == "Revoke approval for “Acme Fake”?")
        await decisions.revoke(approvedDetail, feedback: feedback)
        #expect(
            feedback.current?.message == ProviderTrustCopy.revoked(displayName: "Acme Fake"))
        try await waitFor("the revoked entry in the store", seconds: 20) {
            summary(catalog)?.approval == .revoked
        }
        let revokedCreate = try #require(try await createRefusal(rig, name: "after-revoke"))
        #expect(revokedCreate.code == "provider.revoked")
        #expect(detail("by", of: revokedCreate) == "user")
        let shown = Feedback.failure(revokedCreate)
        #expect(shown.message.hasPrefix("Acme") || shown.message.contains(local))
        #expect(
            shown.detail?.localizedCaseInsensitiveContains("existing instances still start") == true
        )
        let revokedDetail = try await loadedDetail(catalog)
        let banner = try #require(
            ProviderTrustCopy.banner(
                detail: revokedDetail, catalog: catalog.info,
                style: Date.FormatStyle(date: .abbreviated, time: .omitted)))
        #expect(banner.title == "Approval revoked")
        #expect(banner.action == .approveAgain)
        try await rig.start(created)
        try await rig.stop(created)

        // Another client approves again: the store follows through catalog.changed alone.
        let digest = try #require(revokedDetail.local?.packageSha256)
        _ = try await rig.send(
            .providerApprove(
                DecideProviderParams(provider: local, expectedPackageSha256: digest)),
            key: SkeletonRig.newKey())
        try await waitFor("the approval of the other client in the store", seconds: 20) {
            summary(catalog)?.approval == .approved
        }

        // Remove from the panel: the entry leaves the catalog.
        await decisions.remove(try await loadedDetail(catalog), feedback: feedback)
        #expect(
            feedback.current?.message == ProviderTrustCopy.removed(displayName: "Acme Fake"))
        try await waitFor("the entry to leave the store", seconds: 20) {
            catalog.providers?.contains { $0.provider == local } == false
        }
        Evidence.actual(
            "import pending, form and create refused provider.approval_required, stale digest closed the sheet with provider.import_changed, approve opened the gate (form source local_import, create succeeded), revoke refused create with provider.revoked by user while the old instance started and stopped, another client's approve reached the store, remove took the entry away"
        )
    }

    /// `detail` with a package digest that is not the stored one.
    private static func changedDigest(_ detail: ProviderDetail) throws -> ProviderDetail {
        let data = try StacklyJSON.encoder().encode(detail)
        guard var json = try JSONSerialization.jsonObject(with: data) as? [String: Any],
            var local = json["local"] as? [String: Any]
        else {
            throw RigFailure(description: "the detail has no local part")
        }
        local["packageSha256"] = String(repeating: "0", count: 64)
        json["local"] = local
        return try StacklyJSON.decoder().decode(
            ProviderDetail.self, from: JSONSerialization.data(withJSONObject: json))
    }
}
