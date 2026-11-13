import Foundation
import StacklyClient
import StacklyTestSupport
import Testing

@testable import StacklyUI

/// Down from the app's models against a real `stacklyd` (spec 0047, AC-20, AC-21, AC-22), in
/// the pattern of `EnvironmentEndToEndTests`: the live shell's ``EnvironmentStore`` and the
/// ``EnvironmentDownModel`` of the page read `env.preview_down` of an imported ERP
/// environment, open no confirmation for a preview with nothing to do, then send `env.down`
/// as Bring Down does and show its result, with no file of the root changed.
///
/// Only the six official providers are in the bundled catalog, and their runtimes are not in
/// a fresh root, so the environment is imported, never brought up: nothing runs and no lease
/// is held. A Down that stops real instances is covered by the native daemon tests. Without
/// the binaries or the catalog the test ends as BLOCKED (with evidence).
@Suite(.tags(.daemon), .timeLimit(.minutes(3)))
@MainActor
struct EnvironmentDownEndToEndTests {
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

    /// Every file under `folder` with its bytes, without the daemon's logs (the rig's own log
    /// file included), run files, and registry, which a Down writes by design.
    private static func bytes(under folder: String) -> [String: Data] {
        let enumerator = FileManager.default.enumerator(atPath: folder)
        var found: [String: Data] = [:]
        while let path = enumerator?.nextObject() as? String {
            if path.hasPrefix("logs") || path.contains("registry") || path.hasPrefix("run")
                || path.hasSuffix(".sock") || path.hasSuffix(".lock")
                || path.hasSuffix(".stderr") || path.hasSuffix(".stdout")
                || path.hasSuffix(".log")
            {
                continue
            }
            var isFolder: ObjCBool = false
            let full = "\(folder)/\(path)"
            guard FileManager.default.fileExists(atPath: full, isDirectory: &isFolder),
                isFolder.boolValue == false,
                let data = FileManager.default.contents(atPath: full)
            else { continue }
            found[path] = data
        }
        return found
    }

    @Test(
        .evidence(
            covers: ["REQ-022", "NFR-012"],
            expected:
                "against a real stacklyd the GUI's Down model reads env.preview_down of an imported ERP environment, says Nothing to bring down. without a confirmation or env.down, then a Bring Down sends env.down with the empty stop list, its operation succeeds, the result card says erp-development released its lease. Nothing needed to stop., the Lease row says No lease, and no file of the root changes"
        ))
    func theGUIBringsDownAnImportedEnvironmentAgainstARealDaemonAndChangesNoFile() async throws {
        guard let catalog = Self.catalogDir() else {
            try LiveDaemon.unavailable(
                "no bundled catalog; run cargo xtask build-app or set STACKLY_TEST_CATALOG_DIR",
                owner: "feature 47", recordsEvidence: true)
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
        let path = try manifest(in: rig.daemon.root)

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
        await presenter.importPlan()
        let detail = try #require(imported, "phase \(presenter.phase)")
        let id = detail.summary.id
        let instancesBefore = try await rig.listInstances().map(\.id)
        let before = bytes(under: rig.daemon.root)

        // Down: the preview has nothing to do, so no confirmation and no env.down.
        var feedback: [Feedback] = []
        let down = EnvironmentDownModel(store: store)
        down.report = { feedback.append($0) }
        await down.begin(environment: id)
        #expect(down.confirmation == nil)
        #expect(feedback.map(\.message) == [EnvironmentCopy.nothingToBringDown])
        #expect(down.watched.isEmpty, "nothing was sent")

        // Bring Down of that preview, as the confirmation's button does.
        let preview = try await store.previewDown(id)
        #expect(preview.stop.isEmpty)
        #expect(preview.leaseCount == 0)
        #expect(preview.idle.count + preview.keep.count + preview.blocked.count == 0)
        feedback.removeAll()
        await down.bringDown(preview)
        let operation = try #require(down.watched.keys.first, "feedback \(feedback)")
        var ended = try await store.operation(operation)
        var timer = ResponsiveTimer(maxCharge: .milliseconds(70))
        while EnvironmentStore.hasEnded(ended.state) == false {
            timer.lap()
            guard timer.counted < .seconds(60) else {
                throw RigFailure(description: "the Down did not end: \(ended)")
            }
            try await Task.sleep(for: .milliseconds(50))
            ended = try await store.operation(operation)
        }
        #expect(ended.kind == .environmentDown)
        #expect(ended.state == .succeeded, "\(ended)")
        await down.ended(EndedOperation(operation: operation, environment: id))
        #expect(
            feedback.map(\.message) == [
                "erp-development released its lease. Nothing needed to stop."
            ])

        let page = EnvironmentDetailModel(id: id, store: store)
        await page.load(checkManifest: false)
        #expect(page.detail?.lease?.activeCount == 0)
        #expect(
            EnvironmentCopy.leaseText(
                page.detail?.lease, now: .now, calendar: .current, locale: .current,
                timeZone: .current)
                == "No lease. Up takes one on each service’s instance.")
        #expect(try await rig.listInstances().map(\.id) == instancesBefore)
        let after = bytes(under: rig.daemon.root)
        for (file, data) in before {
            #expect(after[file] == data, "\(file) keeps its bytes")
        }
    }
}
