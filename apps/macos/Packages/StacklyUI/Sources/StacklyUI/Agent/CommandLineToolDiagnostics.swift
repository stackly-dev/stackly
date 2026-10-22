import Foundation

/// What the Command Line Tool section of Diagnostics shows, as a pure value so the rows can
/// be tested without a view (spec 0015, *GUI*).
nonisolated public struct CommandLineToolDiagnostics: Hashable, Sendable {
    /// The line to add to a shell profile when Terminal cannot find `stackly`. Shown as text
    /// the user can copy; Stackly never writes it anywhere.
    public static let pathLine = #"export PATH="$HOME/.local/bin:$PATH""#
    /// What introduces ``pathLine``.
    public static let pathNote =
        "If Terminal can’t find stackly, add this line to your shell profile:"

    /// The state of the command link.
    public var command: CommandLineLinkState
    /// Where the link points, as written.
    public var target: String?
    /// The user's home folder path, for `~`.
    public var home: String

    /// Creates the value.
    public init(command: CommandLineLinkState, target: String?, home: String) {
        self.command = command
        self.target = target
        self.home = home
    }

    /// The words of the Status row.
    public static func text(for state: CommandLineLinkState) -> String {
        switch state {
        case .notInstalled: "Not installed"
        case .installed: "Installed"
        case .stale: "Points to another copy of Stackly"
        case .broken: "Points to a copy of Stackly that’s gone"
        case .conflict: "Something else is there. Stackly won’t change it."
        }
    }

    /// The rows: Status, Command, Points to, and PATH note. The `export` line is a row of
    /// its own for Copy Details, and the view shows it next to its Copy button.
    public var rows: [DiagnosticsRow] {
        [
            DiagnosticsRow("Status", text: Self.text(for: command)),
            DiagnosticsRow(
                "Command", code: UserHome.tildePath(home + "/.local/bin/stackly", home: home)),
            target.map { DiagnosticsRow("Points to", code: UserHome.tildePath($0, home: home)) }
                ?? DiagnosticsRow("Points to", text: "Nothing"),
            DiagnosticsRow("PATH note", text: Self.pathNote),
        ]
    }

    /// Which of Install, Repair, and Remove fit the state, in that order.
    public enum Action: String, Sendable, Hashable {
        /// Link the command and the manual pages.
        case install = "Install"
        /// Point the links to this copy of Stackly.
        case repair = "Repair"
        /// Delete the links Stackly made.
        case remove = "Remove"
    }

    /// The actions the state offers: Install when nothing is there, Repair for a stale or
    /// broken link, Remove for any link Stackly made, and none for a conflict.
    public var actions: [Action] {
        switch command {
        case .notInstalled: [.install]
        case .installed: [.remove]
        case .stale, .broken: [.repair, .remove]
        case .conflict: []
        }
    }
}
