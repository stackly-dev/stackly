import Foundation
import StacklyClient
import StacklyTestSupport
import Testing

@testable import StacklyUI

/// The Environments screens' models against a real `stacklyd` (spec 0044, AC-20, AC-21,
/// AC-25 GUI part): the live shell's ``EnvironmentStore`` and the models the pages use read
/// the plan of the ERP manifest, import it, follow the manifest file, and remove it.
///
/// The daemon runs on a root seeded by `skeleton_seed` with the bundled catalog of the app
/// as `STACKLY_CATALOG_DIR` (the six official providers), from `STACKLY_TEST_CATALOG_DIR`
/// or `target/app/Debug/Stackly.app/Contents/Resources/catalog` (`cargo xtask build-app`).
/// Without the binaries or the catalog the test ends as BLOCKED (with evidence).
@Suite(.tags(.daemon), .timeLimit(.minutes(3)))
@MainActor
struct EnvironmentEndToEndTests {
    /// The bundled catalog, or `nil` when it was not built.
    private static func catalogDir() -> String? {
        if let override = ProcessInfo.processInfo.environment["STACKLY_TEST_CATALOG_DIR"],
            override.hasPrefix("/")
        {
            return override
        }
        let built = SourceTree.url(
            of: "target/app/Debug/Stackly.app/Contents/Resources/catalog"
        ).path(percentEncoded: false)
        return FileManager.default.fileExists(atPath: built + "/catalog.json") ? built : nil
    }

    /// A copy of the ERP manifest of spec 0044 in a folder of the test root.
    private static func manifest(in root: String) throws -> String {
        let folder = "\(root)/projects/erp"
        try FileManager.default.createDirectory(
            atPath: folder, withIntermediateDirectories: true)
        let path = "\(folder)/stackly.yaml"
        try FileManager.default.copyItem(
            at: SourceTree.url(of: "crates/core/tests/fixtures/environments/erp-development.yaml"),
            to: URL(filePath: path))
        return path
    }

    /// The files under `folder`, relative to it, without the daemon's logs and registry.
    private static func files(under folder: String) -> Set<String> {
        let enumerator = FileManager.default.enumerator(atPath: folder)
        var found: Set<String> = []
        while let path = enumerator?.nextObject() as? String {
            found.insert(path)
        }
        return found.filter { path in
            path.hasPrefix("logs") == false && path.contains("registry") == false
                && path.hasSuffix(".sock") == false && path.hasSuffix(".lock") == false
                && path.hasPrefix("run") == false
        }
    }

    @Test(
        .evidence(
            covers: ["REQ-020", "AC-020", "AC-080"],
            expected:
                "against a real stacklyd the GUI models read the ERP plan with six installs, creates, and starts, change nothing until Import, import it, see the manifest change, reload it, and remove it with its instances kept"
        ))
    func theGUIPlansImportsReloadsAndRemovesTheERPManifestAgainstARealDaemon() async throws {
        guard let catalog = Self.catalogDir() else {
            try LiveDaemon.unavailable(
                "no bundled catalog; run cargo xtask build-app or set STACKLY_TEST_CATALOG_DIR",
                owner: "feature 44", recordsEvidence: true)
        }
        try await SkeletonRig.run(
            recordsEvidence: true, environment: ["STACKLY_CATALOG_DIR": catalog]
        ) { rig in
            let parts = ShellEndToEndTests.shell(for: rig.daemon)
            do {
                try await Self.walk(rig: rig, model: parts.runtime.model)
            } catch {
                await parts.shutDown()
                throw error
            }
            await parts.shutDown()
        }
    }

    private static func walk(rig: SkeletonRig, model: DaemonModel) async throws {
        try await ShellEndToEndTests.waitUntilRefreshed(model, "the shell to connect and refresh")
        let store = model.environments
        #expect(store.environments == [])
        let path = try manifest(in: rig.daemon.root)
        let before = files(under: rig.daemon.root)
        let instancesBefore = try await rig.listInstances().map(\.id)

        // Import…: the file panel, then env.plan; nothing changes until Import.
        let presenter = EnvironmentPlanPresenter(
            store: store, picker: FixtureFilePicker(path: path))
        var imported: EnvironmentDetail?
        presenter.imported = { imported = $0 }
        presenter.begin()
        try await waitFor("the plan of the ERP manifest", seconds: 30) {
            presenter.plan != nil || presenter.phase == .idle
        }
        if case .failed(let error, _) = presenter.phase {
            throw RigFailure(description: "env.plan failed: \(error.code) \(error.message)")
        }
        let plan = try #require(presenter.plan)
        #expect(plan.validation.valid, "findings \(plan.validation.findings ?? [])")
        #expect(plan.registration == .new)
        let items = plan.items ?? []
        #expect(items.count == 6)
        #expect(items.allSatisfy { $0.action == .create })
        #expect(items.filter { $0.steps?.contains(.installRuntime) == true }.count == 6)
        #expect(items.filter { $0.steps?.contains(.createInstance) == true }.count == 6)
        #expect(items.filter { $0.steps?.contains(.startInstance) == true }.count == 6)
        #expect(items.allSatisfy { $0.trust == .trusted })
        #expect(EnvironmentPlanPresenter.importBlockedReason(plan) == nil)
        #expect(files(under: rig.daemon.root) == before)
        #expect(try await rig.listInstances().map(\.id) == instancesBefore)

        // Import: the environment is registered with no instance, runtime, or data file.
        await presenter.importPlan()
        let detail = try #require(imported, "phase \(presenter.phase)")
        #expect(detail.summary.name == "erp-development")
        #expect(detail.summary.serviceCount == 6)
        #expect(detail.summary.status == .down)
        #expect(files(under: rig.daemon.root) == before)
        #expect(try await rig.listInstances().map(\.id) == instancesBefore)
        try await waitFor("the environment in the store", seconds: 20) {
            store.environments?.map(\.name) == ["erp-development"]
        }

        // The detail page: in sync, Validate, then a changed file and Reload.
        let page = EnvironmentDetailModel(id: detail.summary.id, store: store)
        await page.load(checkManifest: true)
        #expect(page.manifestState == .inSync)
        #expect(page.detail?.services?.allSatisfy { $0.notReady?.reason == .notCreated } == true)
        let valid = await page.validate()
        #expect(valid?.message == EnvironmentCopy.validMessage("erp-development"))
        let text = try String(contentsOfFile: path, encoding: .utf8)
        try text.replacingOccurrences(of: "maxConnections: 200", with: "maxConnections: 201")
            .write(toFile: path, atomically: true, encoding: .utf8)
        await page.load(checkManifest: true)
        #expect(page.manifestState == .changed)
        let reloaded = await page.reloadManifest()
        #expect(reloaded?.tone == .positive, "feedback \(String(describing: reloaded))")
        #expect(page.manifestState == .inSync)

        // Remove: the environment goes, the file stays.
        guard case .removed = await page.remove() else {
            throw RigFailure(description: "env.remove did not remove the environment")
        }
        try await waitFor("the empty store", seconds: 20) { store.environments == [] }
        #expect(FileManager.default.fileExists(atPath: path))
        #expect(try await rig.listInstances().map(\.id) == instancesBefore)
    }
}
