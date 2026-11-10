import AppKit
import Foundation
import StacklyClient

/// Where the person chose to save the bundle, and whether the save panel already asked to
/// replace a file there.
struct BundleDestination: Equatable, Sendable {
    /// The file to write.
    var url: URL
    /// True only when a file is there and the panel asked to replace it.
    var replace: Bool
}

/// Asks the person where to save the bundle (spec 0034, AC-19). The app uses
/// ``SavePanelBundleDestination``; a test answers with a fixed URL.
protocol BundleDestinationChoosing {
    /// Shows the save panel and returns the destination, or `nil` when the person cancels.
    /// - Parameter suggestedName: The file name the panel starts with.
    func choose(suggestedName: String) async -> BundleDestination?
}

/// ``BundleDestinationChoosing`` over `NSSavePanel`, in Downloads. The panel asks before it
/// returns a file that exists, so a file at the chosen URL means the person confirmed Replace.
struct SavePanelBundleDestination: BundleDestinationChoosing {
    func choose(suggestedName: String) async -> BundleDestination? {
        let panel = NSSavePanel()
        panel.nameFieldStringValue = suggestedName
        panel.canCreateDirectories = true
        panel.directoryURL =
            FileManager.default.urls(
                for: .downloadsDirectory, in: .userDomainMask
            ).first
        let url: URL? = await withCheckedContinuation { continuation in
            panel.begin { response in
                continuation.resume(returning: response == .OK ? panel.url : nil)
            }
        }
        guard let url else { return nil }
        return BundleDestination(
            url: url, replace: FileManager.default.fileExists(atPath: url.path))
    }
}

/// The bundle sheet (spec 0034, AC-19): `loading` while the daemon builds the preview,
/// `previewed` with its entries, `exporting` while it copies the zip, then the sheet closes.
/// An expired preview loads a new one with a note; any other error stays in the sheet. The
/// GUI never writes the file itself: the daemon copies the exact bytes of the preview.
@Observable
final class BundleModel {
    /// The preview the person reviews.
    private(set) var preview: BundlePreview?
    /// Whether the daemon is building a preview.
    private(set) var loading = false
    /// Whether the daemon is copying the zip.
    private(set) var exporting = false
    /// The error of the last call.
    private(set) var failure: ErrorBody?
    /// A note above the entries, such as the one after a preview expired.
    private(set) var notice: String?

    @ObservationIgnored private let read: @MainActor (Call) async throws(ClientError) -> Reply

    /// Creates a model that has no preview yet.
    /// - Parameter read: Sends one call to the daemon.
    init(read: @escaping @MainActor (Call) async throws(ClientError) -> Reply) {
        self.read = read
    }

    /// Builds a new preview. A call that is already running makes this one return at once.
    func load() async {
        guard loading == false else { return }
        loading = true
        defer { loading = false }
        do {
            let reply = try await read(.diagnosticsPreviewBundle)
            guard case .diagnosticsPreviewBundle(let fresh) = reply else {
                failure = ClientErrorCatalog.protocolViolation()
                return
            }
            preview = fresh
            failure = nil
        } catch {
            failure = error.body
        }
    }

    /// Copies the preview to `destination`; returns what was written, or `nil` when the
    /// export failed (the error shows in the sheet) or the preview expired (a new one loads).
    func export(to destination: BundleDestination) async -> ExportedBundle? {
        guard let preview, exporting == false else { return nil }
        exporting = true
        defer { exporting = false }
        let params = ExportBundleParams(
            previewId: preview.previewId, destination: destination.url.path,
            replace: destination.replace)
        do {
            let reply = try await read(.diagnosticsExportBundle(params))
            guard case .diagnosticsExportBundle(let exported) = reply else {
                failure = ClientErrorCatalog.protocolViolation()
                return nil
            }
            failure = nil
            notice = nil
            return exported
        } catch {
            if error.body.code == "diagnostics.preview_not_found" {
                notice = DoctorCopy.previewExpired
                self.preview = nil
                failure = nil
                exporting = false
                await load()
                return nil
            }
            failure = error.body
            return nil
        }
    }
}
