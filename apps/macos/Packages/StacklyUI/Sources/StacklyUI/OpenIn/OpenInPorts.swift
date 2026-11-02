import AppKit
import Foundation

/// Finds an installed application by bundle ID (spec 0070, *Value sourcing*). The menu asks it
/// every time it is built, so an app installed or removed since is seen at once.
public protocol AppLocator {
    /// The URL of the installed application with `bundleIdentifier`, or `nil` when there is
    /// none.
    func applicationURL(forBundleIdentifier bundleIdentifier: String) -> URL?
}

/// Opens what an Open In action names: a URL with one application, a URL in the default
/// browser, or a file in Finder (spec 0070, AC-7 to AC-9). The app uses
/// ``SystemWorkspaceOpener``; a test records the calls.
public protocol WorkspaceOpener {
    /// Opens `url` with the application at `application`.
    /// - Throws: The error NSWorkspace gives, such as an application that was removed.
    func open(_ url: URL, withApplicationAt application: URL) async throws
    /// Opens `url` with the default browser.
    /// - Throws: ``OpenInFailure/refused`` when no application takes it.
    func openInBrowser(_ url: URL) throws
    /// Shows `file` selected in a Finder window.
    func reveal(_ file: URL)
}

/// What the Open In menu needs from the system, in one value (spec 0070, *Value sourcing*): the
/// app uses ``system``; a snapshot or a test gives fakes.
public struct OpenInPorts {
    /// Finds installed applications.
    public var locator: any AppLocator
    /// Opens URLs and files.
    public var opener: any WorkspaceOpener
    /// Whether a path exists, for `reveal`.
    public var fileExists: (String) -> Bool

    /// Creates the ports.
    public init(
        locator: any AppLocator, opener: any WorkspaceOpener,
        fileExists: @escaping (String) -> Bool
    ) {
        self.locator = locator
        self.opener = opener
        self.fileExists = fileExists
    }

    /// The ports of the running app: NSWorkspace and the file manager.
    public static var system: OpenInPorts {
        OpenInPorts(
            locator: WorkspaceAppLocator(), opener: SystemWorkspaceOpener(),
            fileExists: { FileManager.default.fileExists(atPath: $0) })
    }
}

/// Why the workspace did not open something.
public enum OpenInFailure: LocalizedError, Equatable {
    /// No application took the URL.
    case refused
    /// The action holds a URL or path that is not one.
    case invalidTarget
    /// No installed application has one of the bundle IDs.
    case applicationMissing

    /// The sentence for the error card.
    public var errorDescription: String? {
        switch self {
        case .refused: "No application opened the address."
        case .invalidTarget: "The address of this action isn’t valid."
        case .applicationMissing: "The application isn’t installed anymore."
        }
    }
}

/// ``AppLocator`` over `NSWorkspace`.
public struct WorkspaceAppLocator: AppLocator {
    /// Creates the locator.
    public init() {}

    /// The URL NSWorkspace knows for `bundleIdentifier`.
    public func applicationURL(forBundleIdentifier bundleIdentifier: String) -> URL? {
        NSWorkspace.shared.urlForApplication(withBundleIdentifier: bundleIdentifier)
    }
}

/// ``WorkspaceOpener`` over `NSWorkspace`. It never asks for Automation and sends no Apple
/// Event: Terminal opens a `.command` file the way a double click does.
public struct SystemWorkspaceOpener: WorkspaceOpener {
    /// Creates the opener.
    public init() {}

    /// Opens `url` with `application` through `NSWorkspace.open(_:withApplicationAt:)`.
    public func open(_ url: URL, withApplicationAt application: URL) async throws {
        _ = try await NSWorkspace.shared.open(
            [url], withApplicationAt: application, configuration: NSWorkspace.OpenConfiguration())
    }

    /// Opens `url` through `NSWorkspace.open(_:)`.
    public func openInBrowser(_ url: URL) throws {
        guard NSWorkspace.shared.open(url) else { throw OpenInFailure.refused }
    }

    /// Selects `file` in Finder through `activateFileViewerSelecting(_:)`.
    public func reveal(_ file: URL) {
        NSWorkspace.shared.activateFileViewerSelecting([file])
    }
}
