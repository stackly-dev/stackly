import Foundation

/// The state of one managed link: the `stackly` command or a manual page (spec 0015,
/// *Instalasi CLI*). The raw values are the words of the spec.
nonisolated public enum CommandLineLinkState: String, Sendable, Equatable, CaseIterable {
    /// Nothing is at the path.
    case notInstalled = "not_installed"
    /// A link Stackly made, to the copy of Stackly that runs now.
    case installed
    /// A link Stackly made, to another copy of Stackly.
    case stale
    /// A link Stackly made whose target is gone.
    case broken
    /// A file, folder, or link that Stackly did not make. Stackly never touches it.
    case conflict
}

/// The state of the `stackly` command and of each manual page in the bundle.
nonisolated public struct CommandLineToolStatus: Sendable, Equatable {
    /// One manual page of the bundle and the state of its link.
    nonisolated public struct ManPage: Sendable, Equatable {
        /// The file name, such as `stackly.1`.
        public var name: String
        /// The state of its link.
        public var state: CommandLineLinkState

        /// Creates the entry.
        public init(name: String, state: CommandLineLinkState) {
            self.name = name
            self.state = state
        }
    }

    /// The state of `~/.local/bin/stackly`.
    public var command: CommandLineLinkState
    /// The target of the command link, written as it is, or `nil` when it is not a link.
    public var commandTarget: String?
    /// The manual pages of the bundle, with the state of their links.
    public var manPages: [ManPage]

    /// Creates the status.
    public init(
        command: CommandLineLinkState, commandTarget: String? = nil, manPages: [ManPage] = []
    ) {
        self.command = command
        self.commandTarget = commandTarget
        self.manPages = manPages
    }

    /// Nothing is installed and the bundle has no manual page.
    public static let notInstalled = CommandLineToolStatus(command: .notInstalled)
}

/// Why installing, repairing, or removing the command failed. Every case names the path it
/// stopped at and writes nothing more.
nonisolated public enum CommandLineToolError: Error, Sendable, Equatable {
    /// The running app has no `Contents/MacOS/stackly-cli`.
    case noBundledTool
    /// A folder is not owned by the user, or others can write to it.
    case folderNotSafe(path: String)
    /// `path` holds something Stackly did not make.
    case conflict(path: String)
    /// A file operation failed at `path`.
    case io(path: String, reason: String)

    /// The text Diagnostics and onboarding show.
    public var message: String {
        switch self {
        case .noBundledTool:
            "This build doesn’t include the stackly command."
        case .folderNotSafe(let path):
            "\(path) must belong to you, and other users must not be able to write to it."
        case .conflict(let path):
            "\(path) already exists and wasn’t made by Stackly. Stackly won’t change it."
        case .io(let path, let reason):
            "Stackly couldn’t change \(path): \(reason)."
        }
    }
}

/// The command line tool as the model sees it (spec 0015, *Banner, pengumuman, dan
/// environment*): the live ``CommandLineTool`` and the in-memory ``FakeCommandLineTool``.
nonisolated protocol CommandLineToolControlling: Sendable {
    /// The user's home folder; `UserHome.tildePath` writes paths against it.
    var home: URL { get }
    /// `<bundle>/Contents/MacOS/stackly-cli` when it exists and can run, else `nil`.
    var bundledTool: URL? { get }
    /// The state of the command and of each manual page.
    @concurrent func status() async -> CommandLineToolStatus
    /// Links the command and the manual pages to the running copy of Stackly.
    @concurrent func install() async throws(CommandLineToolError)
    /// Points every link to the running copy of Stackly, like ``install()``.
    @concurrent func repair() async throws(CommandLineToolError)
    /// Deletes the links Stackly made, and only those.
    @concurrent func remove() async throws(CommandLineToolError)
}
