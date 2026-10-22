import Foundation

/// The three confirmations of the command line tool, with the words of spec 0016
/// (*Kontrol Command Line*). Diagnostics and onboarding share them, so the text has one
/// source. All three are `standard`: they change links Stackly made, not the user's data.
public enum CommandLineToolPrompts {
    /// Install the command and the manual pages.
    ///
    /// - Parameter perform: Runs when the user confirms.
    public static func install(perform: @escaping @MainActor () -> Void) -> ConfirmationRequest {
        ConfirmationRequest(
            title: "Install the stackly command?",
            message:
                "Stackly creates a link at ~/.local/bin/stackly and links its manual pages in ~/.local/share/man/man1. Your PATH and shell files stay as they are.",
            actionTitle: "Install", role: .standard, perform: perform)
    }

    /// Point the links to this copy of Stackly.
    ///
    /// - Parameter perform: Runs when the user confirms.
    public static func repair(perform: @escaping @MainActor () -> Void) -> ConfirmationRequest {
        ConfirmationRequest(
            title: "Repair the stackly command?",
            message:
                "Stackly points ~/.local/bin/stackly and its manual pages to this copy of Stackly. Your PATH and shell files stay as they are.",
            actionTitle: "Repair", role: .standard, perform: perform)
    }

    /// Remove the links Stackly made.
    ///
    /// - Parameter perform: Runs when the user confirms.
    public static func remove(perform: @escaping @MainActor () -> Void) -> ConfirmationRequest {
        ConfirmationRequest(
            title: "Remove the stackly command?",
            message:
                "Stackly deletes only the links it made in ~/.local/bin and ~/.local/share/man/man1. Stackly itself stays installed.",
            actionTitle: "Remove", role: .standard, perform: perform)
    }
}
