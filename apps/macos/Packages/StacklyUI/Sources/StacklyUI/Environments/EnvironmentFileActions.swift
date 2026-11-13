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
/// Reload Manifest, then Lock Versions, Update Lock…, and Copy Environment Variables
/// (spec 0045), then Remove Environment….
enum EnvironmentFileActions {
    /// The sections of the menu.
    /// - Parameters:
    ///   - location: The manifest path, or `nil` when the environment has none: the file
    ///     actions are off with the reason.
    ///   - ports: Opens and reveals the file.
    ///   - reload: Reload Manifest, or `nil` when there is no file.
    ///   - lock: The lock and export items, from `lockSection`.
    ///   - remove: Remove Environment…, or `nil` while the page has no summary.
    ///   - isReloading: Whether Reload runs.
    @MainActor
    static func sections(
        location: String?, ports: ManifestFilePorts, reload: (@MainActor () -> Void)?,
        lock: [StacklyAction] = [], remove: StacklyAction?, isReloading: Bool
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
        return [file, lock, remove.map { [$0] } ?? []].filter { $0.isEmpty == false }
    }

    /// Lock Versions, Update Lock…, and Copy Environment Variables (spec 0045, AC-18). The two
    /// lock items write the lock file, so they are gated like mutations and need a manifest;
    /// Update Lock… confirms first. Copy reads the registry and works without a manifest.
    /// - Parameters:
    ///   - location: The manifest path, or `nil`.
    ///   - lockFile: The lock file name, for the confirmation.
    ///   - isLocking: Whether a lock runs.
    ///   - isExporting: Whether Copy runs.
    ///   - lock: Runs Lock Versions.
    ///   - update: Runs Update Lock… after the confirmation.
    ///   - copy: Runs Copy Environment Variables.
    @MainActor
    static func lockSection(
        location: String?, lockFile: String, isLocking: Bool, isExporting: Bool,
        lock: @escaping @MainActor () -> Void, update: @escaping @MainActor () -> Void,
        copy: @escaping @MainActor () -> Void
    ) -> [StacklyAction] {
        let missing = location == nil ? "This environment has no manifest file." : nil
        return [
            StacklyAction(
                id: "environment.lockVersions", title: EnvironmentCopy.lockVersions,
                mutates: true, disabledReason: missing, isBusy: isLocking, perform: lock),
            StacklyAction(
                id: "environment.updateLock", title: EnvironmentCopy.updateLock,
                role: .standard(
                    confirmation: ActionConfirmation(
                        title: EnvironmentCopy.updateLockTitle,
                        message: EnvironmentCopy.updateLockMessage(lockFile),
                        actionTitle: EnvironmentCopy.updateLockAction)),
                mutates: true, disabledReason: missing, isBusy: isLocking, perform: update),
            StacklyAction(
                id: "environment.copyVariables", title: EnvironmentCopy.copyVariables,
                mutates: false, isBusy: isExporting, perform: copy),
        ]
    }
}
