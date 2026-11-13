import AppKit
import Foundation

/// What the More menu of an environment asks of the system for its manifest file (spec 0044,
/// AC-21): open it in the app macOS uses for the file, and show it in the Finder. Stackly has
/// no YAML editor. The app uses ``system``; previews and tests record the calls.
struct ManifestFilePorts {
    /// Opens the file in its default app.
    var open: @MainActor (URL) -> Void
    /// Shows the file in the Finder.
    var reveal: @MainActor (URL) -> Void

    /// NSWorkspace.
    static let system = ManifestFilePorts(
        open: { NSWorkspace.shared.open($0) },
        reveal: { NSWorkspace.shared.activateFileViewerSelecting([$0]) })
}

/// The More menu of an environment's detail page: Open Manifest, Reveal in Finder, Copy Path,
/// Reload Manifest, then Remove Environment….
enum EnvironmentFileActions {
    /// The sections of the menu.
    /// - Parameters:
    ///   - location: The manifest path, or `nil` when the environment has none: the file
    ///     actions are off with the reason.
    ///   - ports: Opens and reveals the file.
    ///   - reload: Reload Manifest, or `nil` when there is no file.
    ///   - remove: Remove Environment…, or `nil` while the page has no summary.
    ///   - isReloading: Whether Reload runs.
    @MainActor
    static func sections(
        location: String?, ports: ManifestFilePorts, reload: (@MainActor () -> Void)?,
        remove: StacklyAction?, isReloading: Bool
    ) -> [[StacklyAction]] {
        let missing = location == nil ? "This environment has no manifest file." : nil
        let url = location.map { URL(fileURLWithPath: $0) }
        var file: [StacklyAction] = [
            StacklyAction(
                id: "environment.openManifest", title: "Open Manifest", mutates: false,
                disabledReason: missing, perform: { if let url { ports.open(url) } }),
            StacklyAction(
                id: "environment.revealManifest", title: "Reveal in Finder", mutates: false,
                disabledReason: missing, perform: { if let url { ports.reveal(url) } }),
            StacklyAction(
                id: "environment.copyPath", title: "Copy Path", mutates: false,
                disabledReason: missing,
                perform: {
                    guard let location else { return }
                    let confirmation = CopyConfirmation()
                    Task { await confirmation.copy(location) }
                }),
        ]
        file.append(
            StacklyAction(
                id: "environment.reloadManifest", title: "Reload Manifest", mutates: true,
                disabledReason: missing, isBusy: isReloading,
                perform: { reload?() }))
        return [file, remove.map { [$0] } ?? []].filter { $0.isEmpty == false }
    }
}
