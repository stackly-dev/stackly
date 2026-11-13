import AppKit
import Foundation
import UniformTypeIdentifiers

/// Asks the person for one file (spec 0044, *Import*). The app uses ``OpenPanelFilePicker``; a
/// test answers with a fixed path.
protocol FilePicking {
    /// Shows the file panel and returns the chosen file's path, or `nil` when the person
    /// cancels.
    /// - Parameters:
    ///   - message: The line at the top of the panel.
    ///   - extensions: The file name extensions the panel offers, such as `yaml` and `yml`.
    ///   - startingAt: The folder the panel opens in, when there is one.
    func chooseFile(message: String, extensions: [String], startingAt: String?) async -> String?
}

/// ``FilePicking`` over `NSOpenPanel`: one file, never a folder.
struct OpenPanelFilePicker: FilePicking {
    func chooseFile(message: String, extensions: [String], startingAt: String?) async -> String? {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = false
        panel.canChooseFiles = true
        panel.allowsMultipleSelection = false
        panel.canCreateDirectories = false
        panel.allowedContentTypes = extensions.compactMap { UTType(filenameExtension: $0) }
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
