import Foundation
import StacklyClient

/// The words of Restart Daemon: the Settings row, the confirmation sheet, the banners, and
/// the toast (spec 0030 AC-16). Pure functions of the contract values, so every sentence can
/// be tested without a view.
nonisolated public enum RestartCopy {
    /// The title of the Settings button and the sheet.
    public static let buttonTitle = "Restart Daemon"
    /// The title of the banner while the daemon is down and coming back.
    public static let restartingTitle = "stacklyd is restarting"
    /// What a row says about an instance the restart stopped.
    public static let rowMark = "Stopped for daemon restart"
    /// The sheet's note while the daemon waits for the operations in flight.
    public static let waitingForOperations = "Waiting for operations to finish"
    /// The subtitle of the Settings row for a daemon outside launchd.
    public static let manualSubtitle = "Runs outside launchd, so it can't restart itself"
    /// The subtitle of the Settings row while the app has no connection.
    public static let disconnectedSubtitle = "Disconnected · reconnecting"
    /// The reason the button is off while a restart runs.
    public static let inProgressReason = "A restart is already in progress."
    /// The reason the button is off without a connection.
    public static let disconnectedReason = "stacklyd is not connected."
    /// What the sheet says when the daemon changed its plan between the preview and the
    /// confirmation.
    public static let planChanged =
        "The instances changed while this sheet was open. Review the new plan before you restart."

    /// "Connected over Unix socket · protocol 1.2".
    /// - Parameter protocolText: The protocol version as the footer writes it.
    public static func connectedSubtitle(protocolText: String) -> String {
        "Connected over Unix socket · protocol \(protocolText)"
    }

    /// Why an instance keeps running or stops, in the words of the sheet.
    public static func reason(_ reason: PlanReason) -> String {
        switch reason {
        case .handoverProven: "Its process survives the restart."
        case .handoverUnproven: "Its provider can't hand the process over yet."
        case .respawnPending: "It is waiting to start and carries on afterwards."
        case .identityUnproven: "Its process is not proven to be its own, so it is left alone."
        case .supervisorUnavailable: "The registry is in recovery mode, so nothing is stopped."
        case .unrecognized: "Decided by the daemon."
        }
    }

    /// The name of an operation that blocks the restart, in the words of the sheet.
    public static func operation(_ kind: OperationKind) -> String {
        switch kind {
        case .installRuntime: "Installing a runtime"
        case .createInstance: "Creating an instance"
        case .startInstance: "Starting an instance"
        case .stopInstance: "Stopping an instance"
        case .restartInstance: "Restarting an instance"
        default: kind.rawValue.replacingOccurrences(of: "_", with: " ").capitalizedFirst
        }
    }

    /// "1 instance" or "3 instances".
    public static func instances(_ count: Int) -> String {
        count == 1 ? "1 instance" : "\(count) instances"
    }

    /// The toast after a restart finished (spec 0030 AC-16).
    /// - Parameters:
    ///   - adopted: How many instances the new daemon adopted.
    ///   - restored: How many it started again.
    public static func toast(adopted: Int, restored: Int) -> String {
        "Daemon restarted. \(instances(adopted)) adopted, \(restored) started again."
    }

    /// A span in seconds as the banner writes it: "0.4 s" under ten seconds, "12 s" above,
    /// and "2 min 5 s" from a minute.
    public static func duration(_ seconds: TimeInterval) -> String {
        let seconds = max(0, seconds)
        if seconds < 10 { return String(format: "%.1f s", seconds) }
        if seconds < 60 { return "\(Int(seconds.rounded())) s" }
        let whole = Int(seconds.rounded())
        return "\(whole / 60) min \(whole % 60) s"
    }

    /// The reason in words of a plan that closed without finishing.
    public static func closeReason(_ reason: PlanCloseReason?) -> String {
        switch reason {
        case .stopTimeout: "An instance did not stop in time."
        case .bootChanged: "The Mac started again before the restart finished."
        case .unrecognized, nil: "The daemon closed the plan."
        }
    }

    /// The row of an aborted plan for an instance it stopped and is starting again.
    public static let stoppedStartingAgain = "Stopped, starting again."

    /// The row of an aborted plan for an instance that did not stop.
    public static let didNotStopStillRunning = "Did not stop, still running."

    /// Why an instance failed to come back, from the plan's failure code.
    public static func failure(_ code: String?) -> String {
        guard let code else { return "Did not come back." }
        return "Did not come back (\(code))."
    }
}

extension String {
    /// The string with its first letter in upper case.
    fileprivate nonisolated var capitalizedFirst: String {
        prefix(1).uppercased() + dropFirst()
    }
}
