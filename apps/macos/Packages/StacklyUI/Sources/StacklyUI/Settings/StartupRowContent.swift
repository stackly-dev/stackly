import Foundation

/// What the Start stacklyd at login row says and allows, from the background agent's state
/// and the number of instances set to start at login (spec 0032 AC-15). It is a pure
/// function, so the wording is tested without a view.
struct StartupRowContent: Equatable {
    /// The state of the agent in a few words.
    var stateText: String
    /// The line under the row: what the next login does, or why it won’t start anything.
    var note: String?
    /// Whether the toggle shows on: the agent is registered.
    var isOn: Bool
    /// Why the toggle cannot be used now, or `nil` when it can.
    var toggleDisabledReason: String?
    /// Whether the row offers Open Login Items.
    var showsOpenLoginItems: Bool

    /// The sentence for an agent that is on: what the next login does.
    /// - Parameter atLoginCount: How many instances have the policy `at_login`.
    static func nextLogin(atLoginCount: Int) -> String {
        switch atLoginCount {
        case 0: "At your next login, stacklyd starts."
        case 1: "At your next login, stacklyd starts, then 1 instance set to start at login."
        default:
            "At your next login, stacklyd starts, then \(atLoginCount) instances set to start at login."
        }
    }

    /// The row for `assessment`.
    /// - Parameters:
    ///   - assessment: The agent's state, or `nil` before the first answer.
    ///   - atLoginCount: How many instances have the policy `at_login`.
    ///   - isWorking: Whether a Turn On or Start call is out.
    ///   - isTurningOff: Whether Turn Off is reading its preview or running.
    static func make(
        assessment: AgentAssessment?, atLoginCount: Int, isWorking: Bool, isTurningOff: Bool = false
    ) -> StartupRowContent {
        guard let assessment else {
            return StartupRowContent(
                stateText: "Checking…", note: nil, isOn: false,
                toggleDisabledReason: nil, showsOpenLoginItems: false)
        }
        let text = AgentDiagnostics.text(for: assessment)
        let busy = isWorking || isTurningOff ? "Working on it…" : nil
        switch assessment.state {
        case .enabled:
            return StartupRowContent(
                stateText: text, note: nextLogin(atLoginCount: atLoginCount), isOn: true,
                toggleDisabledReason: busy, showsOpenLoginItems: false)
        case .requiresApproval:
            return StartupRowContent(
                stateText: text, note: AutostartText.agentNeedsApproval, isOn: true,
                toggleDisabledReason: busy ?? StartupCopy.removeInLoginItems,
                showsOpenLoginItems: true)
        case .notRegistered:
            return StartupRowContent(
                stateText: text, note: AutostartText.agentOff, isOn: false,
                toggleDisabledReason: busy, showsOpenLoginItems: false)
        case .disabled:
            return StartupRowContent(
                stateText: text, note: AutostartText.agentOff, isOn: false,
                toggleDisabledReason: StartupCopy.turnOnInLoginItems, showsOpenLoginItems: true)
        case .notFound:
            return StartupRowContent(
                stateText: text, note: AutostartText.agentOff, isOn: false,
                toggleDisabledReason: StartupCopy.notFound, showsOpenLoginItems: false)
        case .unavailable:
            return StartupRowContent(
                stateText: text, note: nil, isOn: false, toggleDisabledReason: text,
                showsOpenLoginItems: false)
        }
    }
}

/// The words of the Startup group.
enum StartupCopy {
    /// The note of the Open Stackly at login row.
    static let openAtLoginNote = "Opens in the menu bar, without a window."
    /// The line under Open Stackly at login while macOS waits for the user.
    static let openAtLoginApproval = "Allow Stackly in Login Items to finish."
    /// Why the toggle cannot turn off an agent that macOS still waits to approve: Turn Off
    /// stops the running daemon first, and there is none to stop.
    static let removeInLoginItems =
        "To remove it, turn Stackly off in System Settings > General > Login Items & Extensions."
    /// Why the toggle cannot turn the agent on again after the user turned it off in macOS.
    static let turnOnInLoginItems =
        "Turn on Stackly in System Settings > General > Login Items & Extensions."
    /// Why the toggle cannot turn the agent on for a copy macOS cannot find.
    static let notFound =
        "Move Stackly to the Applications folder, or install it again."
}
