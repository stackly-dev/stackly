import AppKit
import Foundation

/// Asks the person for a folder (spec 0032 AC-19). The app uses ``OpenPanelFolderPicker``; a
/// test answers with a fixed path.
protocol FolderPicking {
    /// Shows the folder panel and returns the chosen folder's path, or `nil` when the person
    /// cancels.
    /// - Parameters:
    ///   - message: The line at the top of the panel.
    ///   - startingAt: The folder the panel opens in, when there is one.
    func chooseFolder(message: String, startingAt: String?) async -> String?
}

/// ``FolderPicking`` over `NSOpenPanel`: one folder, never a file, with New Folder.
struct OpenPanelFolderPicker: FolderPicking {
    func chooseFolder(message: String, startingAt: String?) async -> String? {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.allowsMultipleSelection = false
        panel.canCreateDirectories = true
        panel.message = message
        panel.prompt = "Choose"
        if let startingAt {
            panel.directoryURL = URL(fileURLWithPath: startingAt)
        }
        return await withCheckedContinuation { continuation in
            panel.begin { response in
                continuation.resume(returning: response == .OK ? panel.url?.path : nil)
            }
        }
    }
}
