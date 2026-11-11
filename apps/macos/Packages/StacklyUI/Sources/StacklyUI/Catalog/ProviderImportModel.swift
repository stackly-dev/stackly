import Foundation
import Observation
import StacklyClient

/// The import of a provider folder (spec 0037, *Layar*, AC-15): the folder panel, then
/// `provider.preview_import`, then the Import Provider sheet, then `provider.import` with
/// the `packageSha256` the sheet showed.
/// Preview only reads; nothing runs until the person approves the provider later. The
/// Catalog page and the Settings window each own one model over the same ``CatalogStore``.
@MainActor
@Observable
final class ProviderImportModel {
    /// Where the import is.
    enum Phase: Equatable {
        /// No sheet.
        case idle
        /// The folder panel is open.
        case choosing
        /// The daemon reads the folder.
        case loading(path: String)
        /// The daemon read the folder; the sheet shows what it found.
        case preview(ImportPreview, path: String)
        /// The folder can't be read.
        case unreadable(ErrorBody, path: String)
        /// `provider.import` is on its way.
        case importing(ImportPreview, path: String)
        /// The import did not go through; the sheet keeps the preview and says why.
        case failed(ImportPreview, path: String, error: ErrorBody)
    }

    /// Where the import is.
    private(set) var phase: Phase = .idle

    @ObservationIgnored private let picker: any FolderPicking
    @ObservationIgnored private let call: (Call) async throws(ClientError) -> Reply
    @ObservationIgnored private let send: (Call, String) async -> MutationOutcome
    @ObservationIgnored private let imported: @MainActor (ProviderDetail) async -> Void
    /// The key of the import the person asked for, kept when the outcome is unknown so a
    /// retry is the same action.
    @ObservationIgnored private var key: String?
    @ObservationIgnored private let newKey: () -> String

    /// Creates a model.
    /// - Parameters:
    ///   - picker: Asks the person for a folder.
    ///   - call: Sends a read to the daemon (`provider.preview_import`).
    ///   - send: Sends a mutation with its idempotency key (`provider.import`).
    ///   - newKey: Makes an idempotency key.
    ///   - imported: Runs after an import went through, with the detail the daemon sent:
    ///     the owner selects the entry and shows the feedback.
    init(
        picker: any FolderPicking,
        call: @escaping (Call) async throws(ClientError) -> Reply,
        send: @escaping (Call, String) async -> MutationOutcome,
        newKey: @escaping () -> String = { MutationPath.newKey() },
        imported: @escaping @MainActor (ProviderDetail) async -> Void
    ) {
        self.picker = picker
        self.call = call
        self.send = send
        self.newKey = newKey
        self.imported = imported
    }

    /// Whether the sheet shows.
    var isPresented: Bool {
        switch phase {
        case .idle, .choosing: false
        case .loading, .preview, .unreadable, .importing, .failed: true
        }
    }

    /// The preview the sheet shows, when there is one.
    var preview: ImportPreview? {
        switch phase {
        case .preview(let preview, _), .importing(let preview, _), .failed(let preview, _, _):
            preview
        case .idle, .choosing, .loading, .unreadable:
            nil
        }
    }

    /// Opens the folder panel, then reads the chosen folder. Cancel in the panel leaves no
    /// sheet.
    func begin() async {
        guard phase == .idle || isPresented else { return }
        phase = .choosing
        guard
            let path = await picker.chooseFolder(
                message: ProviderTrustCopy.folderPanelMessage, startingAt: nil)
        else {
            phase = .idle
            return
        }
        await read(path)
    }

    /// Reads `path` again: the sheet's Preview Again after a failed import.
    func previewAgain() async {
        switch phase {
        case .failed(_, let path, _), .preview(_, let path), .unreadable(_, let path):
            await read(path)
        case .idle, .choosing, .loading, .importing:
            return
        }
    }

    /// Reads `path` with `provider.preview_import`.
    func read(_ path: String) async {
        key = nil
        phase = .loading(path: path)
        do throws(ClientError) {
            let reply = try await call(.providerPreviewImport(PreviewImportParams(path: path)))
            guard case .loading(let current) = phase, current == path else { return }
            guard case .providerPreviewImport(let preview) = reply else {
                phase = .unreadable(ClientErrorCatalog.protocolViolation(), path: path)
                return
            }
            phase = .preview(preview, path: path)
        } catch {
            guard case .loading(let current) = phase, current == path else { return }
            phase = .unreadable(error.body, path: path)
        }
    }

    /// Whether Import can run for `preview`: the package is importable and is not the same
    /// bytes as an entry whose stored copy is fine.
    static func canImport(_ preview: ImportPreview) -> Bool {
        guard preview.verdict.importable, preview.packageSha256 != nil else { return false }
        if let existing = preview.existing, existing.samePackage, existing.storedCopyValid {
            return false
        }
        return true
    }

    /// Sends `provider.import` with the digest the sheet shows. A daemon that refuses,
    /// or a lost connection, keeps the sheet with the reason; a retry after an unknown
    /// outcome reuses the key.
    func importPackage() async {
        guard case .preview(let preview, let path) = phase, Self.canImport(preview),
            let digest = preview.packageSha256
        else { return }
        let key = self.key ?? newKey()
        self.key = key
        phase = .importing(preview, path: path)
        let outcome = await send(
            .providerImport(ImportProviderParams(path: path, expectedPackageSha256: digest)), key)
        switch outcome {
        case .completed(.providerImport(let detail)):
            self.key = nil
            phase = .idle
            await imported(detail)
        case .completed:
            self.key = nil
            phase = .failed(preview, path: path, error: ClientErrorCatalog.protocolViolation())
        case .rejected(let body), .notSent(let body):
            self.key = nil
            phase = .failed(preview, path: path, error: body)
        case .outcomeUnknown(let body, _):
            phase = .failed(preview, path: path, error: body)
        }
    }

    /// Closes the sheet. A request on its way is not cancelled; its answer is dropped.
    func cancel() {
        key = nil
        phase = .idle
    }
}
