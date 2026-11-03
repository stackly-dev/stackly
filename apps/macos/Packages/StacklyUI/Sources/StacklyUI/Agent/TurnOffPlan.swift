import Foundation
import StacklyClient

/// The words and the state of the Turn Off confirmation in Diagnostics (spec 0031, AC-19 and
/// AC-20). It is built from `instance.preview_stop_all` and the number of instances set to start
/// at login, so the dialog lists exactly what the daemon will stop.
nonisolated public struct TurnOffPlan: Sendable, Equatable {
    /// The title of the confirmation.
    public let title = "Turn Off Background Agent?"
    /// The body of the confirmation, with the skipped instances when there are any.
    public let message: String
    /// Whether the Turn Off button is enabled: not while an instance the daemon must skip runs.
    public let canTurnOff: Bool

    /// Builds the plan.
    /// - Parameters:
    ///   - preview: What `instance.preview_stop_all` found.
    ///   - atLoginCount: How many instances have the policy `at_login`, from `instance.list`.
    public init(preview: StopAllPreview, atLoginCount: Int) {
        let stopping = preview.items.count + preview.alreadyStopping.count
        var text =
            "\(Self.count(stopping, "running instance", "running instances")) will stop. "
            + "\(Self.count(atLoginCount, "instance", "instances")) set to start at login "
            + "won’t start until you turn it on again."
        if preview.skipped.isEmpty == false {
            let names = preview.skipped
                .map { "\($0.name) · \(Self.reason(of: $0.code))" }
                .joined(separator: ", ")
            text += " Turn Off can’t finish while these instances run: \(names)."
        }
        self.message = text
        self.canTurnOff = preview.skipped.isEmpty
    }

    /// The sentence for a Turn Off the daemon answered with `exiting: false`.
    /// - Parameters:
    ///   - result: What the daemon stopped, failed on, and skipped.
    ///   - names: The names of the instances by ID.
    public static func refusal(result: StopAllResult, names: [UUID: String]) -> String {
        let blocking = (result.failed + result.skipped).map {
            "\(names[$0.instanceId] ?? "an instance") · \(reason(of: $0.code))"
        }
        var text = "Background agent is still on. Stopped \(result.stopped.count) instances."
        if blocking.isEmpty == false {
            text += " These couldn’t be stopped: \(blocking.joined(separator: ", "))."
        }
        return text
    }

    private static func count(_ number: Int, _ one: String, _ many: String) -> String {
        number == 1 ? "1 \(one)" : "\(number) \(many)"
    }

    /// A short reason for the error code the daemon gives an instance it leaves alone.
    static func reason(of code: String) -> String {
        switch code {
        case "supervisor.stop_requires_override": "an environment uses it"
        case "supervisor.identity_unproven": "it can’t be confirmed as running"
        case "operation.recovery_required": "an operation needs recovery"
        case "supervisor.stop_timeout": "it didn’t stop in time"
        case "operation.cancelled": "the stop was cancelled"
        default: code
        }
    }
}
