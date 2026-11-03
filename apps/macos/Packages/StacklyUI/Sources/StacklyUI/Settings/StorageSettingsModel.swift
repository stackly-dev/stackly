import Foundation
import Observation
import StacklyClient

/// The actions of the Storage tab: Change… (a folder panel, then a confirmation, then
/// `storage.set_location`), Use Default, and Reveal in Finder (spec 0032 AC-19).
@Observable
final class StorageSettingsModel {
    /// The confirmation shown over the tab, or `nil`.
    var confirmation: ConfirmationRequest?

    @ObservationIgnored private let store: SettingsStore
    @ObservationIgnored private let picker: any FolderPicking
    @ObservationIgnored private let reveal: (URL) -> Void

    /// Creates the model.
    /// - Parameters:
    ///   - store: The daemon's settings.
    ///   - picker: Asks for a folder.
    ///   - reveal: Shows a folder in Finder.
    init(
        store: SettingsStore, picker: any FolderPicking = OpenPanelFolderPicker(),
        reveal: @escaping (URL) -> Void = { OpenInPorts.system.opener.reveal($0) }
    ) {
        self.store = store
        self.picker = picker
        self.reveal = reveal
    }

    /// Change…: asks for a folder and, once the person picked one, for a confirmation that
    /// says what the new folder is used for and what stays where it is.
    func change(_ kind: LocationKind, currentPath: String) async {
        guard
            let path = await picker.chooseFolder(
                message: "Choose a folder for \(LocationRowContent.title(of: kind).lowercased()).",
                startingAt: currentPath)
        else { return }
        confirmation = ConfirmationRequest(
            title: "Use “\(path)” for \(LocationRowContent.title(of: kind).lowercased())?",
            message: LocationRowContent.confirmation(for: kind), actionTitle: "Use Folder",
            role: .standard,
            perform: { [store] in
                Task { await store.setLocation(kind, to: .path(path)) }
            })
    }

    /// Use Default: puts the default of `kind` back on the location that comes with Stackly.
    /// Nothing is moved or deleted, so it needs no confirmation.
    func useDefault(_ kind: LocationKind) async {
        await store.setLocation(kind, to: .builtIn)
    }

    /// Reveal in Finder.
    func revealInFinder(_ path: String) {
        reveal(URL(fileURLWithPath: path))
    }
}
