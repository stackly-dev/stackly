import Foundation
import StacklyClient
import StacklyTestSupport
import Testing

@testable import StacklyUI

/// The bundle sheet's model (spec 0034, AC-19): the preview first, the export of exactly that
/// preview with the path of the save panel, the expired preview, and errors that stay in the
/// sheet. The daemon is a script fed with the contract fixtures; nothing writes a file.
@Suite(.timeLimit(.minutes(1)))
struct BundleModelTests {
    final class Script {
        var replies: [Result<Reply, ClientError>] = []
        var calls: [Call] = []

        func read(_ call: Call) async throws(ClientError) -> Reply {
            calls.append(call)
            guard replies.isEmpty == false else {
                throw .notSent(DoctorModelTests.Script.error("ipc.not_connected"))
            }
            return try replies.removeFirst().get()
        }
    }

    let script = Script()

    func makeModel() -> BundleModel {
        let script = script
        return BundleModel(read: { call throws(ClientError) in try await script.read(call) })
    }

    func preview() throws -> Reply {
        try ContractFixture.reply("frame-response-diagnostics-preview-bundle")
    }

    func exported() throws -> Reply {
        try ContractFixture.reply("frame-response-diagnostics-export-bundle")
    }

    let destination = BundleDestination(
        url: URL(filePath: "/Users/dev/Downloads/stackly-diagnostics-20261110T080000Z.zip"),
        replace: true)

    @Test(
        .evidence(
            covers: ["REQ-019", "AC-019"],
            expected:
                "The sheet first asks for diagnostics.preview_bundle and lists every entry before anything is exported"
        ))
    func theSheetPreviewsFirst() async throws {
        let model = makeModel()
        script.replies = [.success(try preview())]
        await model.load()
        #expect(script.calls == [.diagnosticsPreviewBundle])
        let preview = try #require(model.preview)
        #expect(preview.entries?.count == 9)
        #expect(preview.entries?.first?.path == "README.txt")
        #expect(model.failure == nil)
        #expect(model.loading == false)
    }

    @Test(
        .evidence(
            covers: ["REQ-019", "AC-019"],
            expected:
                "Export sends the preview ID, the save panel's path as is, and replace only when the panel confirmed it"
        ))
    func exportSendsThePreviewAndThePanelPath() async throws {
        let model = makeModel()
        script.replies = [.success(try preview()), .success(try exported())]
        await model.load()
        let result = await model.export(to: destination)
        #expect(result?.path == "/Users/dev/Downloads/stackly-diagnostics-20261110T080000Z.zip")
        guard case .diagnosticsExportBundle(let params) = try #require(script.calls.last) else {
            Issue.record("not an export call")
            return
        }
        #expect(params.previewId == model.preview?.previewId)
        #expect(params.destination == destination.url.path)
        #expect(params.replace == true)

        script.replies = [.success(try exported())]
        _ = await model.export(
            to: BundleDestination(url: destination.url, replace: false))
        guard case .diagnosticsExportBundle(let again) = try #require(script.calls.last) else {
            Issue.record("not an export call")
            return
        }
        #expect(again.replace == false)
    }

    @Test(
        .evidence(
            covers: ["REQ-019", "AC-019"],
            expected:
                "An expired preview loads a new one with The preview expired. Review the new one before you export., and exports nothing"
        ))
    func anExpiredPreviewLoadsANewOne() async throws {
        let model = makeModel()
        script.replies = [
            .success(try preview()),
            .failure(
                .rejected(try ContractFixture.error("frame-error-diagnostics-preview-not-found"))),
            .success(try preview()),
        ]
        await model.load()
        let result = await model.export(to: destination)
        #expect(result == nil)
        #expect(script.calls.count == 3)
        #expect(script.calls.last == .diagnosticsPreviewBundle)
        #expect(model.notice == "The preview expired. Review the new one before you export.")
        #expect(model.preview != nil)
        #expect(model.failure == nil)
        #expect(model.exporting == false)
    }

    @Test(
        .evidence(
            covers: ["REQ-019", "AC-019"],
            expected: "Another export error stays in the sheet with the preview kept"
        ))
    func anotherErrorStaysInTheSheet() async throws {
        let model = makeModel()
        script.replies = [
            .success(try preview()),
            .failure(
                .rejected(try ContractFixture.error("frame-error-diagnostics-destination-exists"))),
        ]
        await model.load()
        let result = await model.export(to: destination)
        #expect(result == nil)
        #expect(model.failure?.code == "diagnostics.destination_exists")
        #expect(model.preview != nil)
        #expect(script.calls.count == 2)
    }

    @Test(
        .evidence(
            covers: ["REQ-019"],
            expected: "A preview that fails leaves no preview, so nothing can be exported"
        ))
    func aFailedPreviewExportsNothing() async throws {
        let model = makeModel()
        script.replies = [
            .failure(.rejected(try ContractFixture.error("frame-error-diagnostics-bundle-failed")))
        ]
        await model.load()
        #expect(model.preview == nil)
        #expect(model.failure?.code == "diagnostics.bundle_failed")
        #expect(await model.export(to: destination) == nil)
        #expect(script.calls == [.diagnosticsPreviewBundle])
    }

    @Test(
        .evidence(
            covers: ["REQ-019"],
            expected:
                "Each entry shows its size, its lines when it is a log, and truncated when cut"
        ))
    func sizesReadLikeFinder() throws {
        guard case .diagnosticsPreviewBundle(let preview) = try preview() else {
            Issue.record("not a preview")
            return
        }
        let entries = preview.entries ?? []
        let english = Locale(identifier: "en_US")
        let log = try #require(entries.first { $0.path == "logs/stacklyd.log" })
        #expect(BundleSizeText.entry(log, locale: english) == "2,000 lines · 412 kB · truncated")
        let readme = try #require(entries.first { $0.path == "README.txt" })
        #expect(BundleSizeText.entry(readme, locale: english) == "2 kB")
        #expect(
            BundleSizeText.summary(preview, locale: english) == "9 files · 590 kB · 101 kB as a zip"
        )
    }
}
