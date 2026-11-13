import Foundation
import StacklyClient

/// The words of the menu bar popover that are not about Stop All (spec 0032, *Popover menu
/// bar*).
nonisolated public enum MenuBarCopy {
    /// The title of the popover.
    public static let title = "Stackly"
    /// The Services section when there is no instance.
    public static let noInstances = "No instances yet."
    /// The label next to the Services title while the daemon cannot be reached.
    public static let lastKnown = "last known"
    /// The sub line of an operation that has no percent.
    public static let working = "Working…"
    /// Open Stackly.
    public static let openStackly = "Open Stackly"
    /// Stop All Services….
    public static let stopAllServices = "Stop All Services…"
    /// Settings….
    public static let settings = "Settings…"
    /// Close Window.
    public static let closeWindow = "Close Window"
    /// Quit Stackly.
    public static let quit = "Quit Stackly"
    /// Why Stop All Services… cannot run while the daemon cannot be reached.
    public static let unreachable = "stacklyd isn’t reachable."

    /// The row that opens the Instances page when there are more than eight instances.
    /// - Parameter total: How many instances there are.
    public static func showAll(_ total: Int) -> String {
        "Show all \(total) in Stackly…"
    }

    /// "{n} running", for the header.
    public static func running(_ count: Int) -> String { "\(count) running" }

    /// "{n} failed", for the header.
    public static func failed(_ count: Int) -> String { "\(count) failed" }
}

/// The copy of the Stop All preview and of its result (spec 0032, *Copy Stop All*). The CLI
/// writes the same words for `stackly instance stop --all`.
nonisolated public enum StopAllCopy {
    /// The title of the preview.
    public static let title = "Stop All Services?"
    /// The line under the list, always.
    public static let dataKept = "Data is kept."
    /// The preview when no instance would stop.
    public static let nothingRunning = "Nothing is running."
    /// The Cancel button, the default one.
    public static let cancel = "Cancel"
    /// The button that stops.
    public static let confirm = "Stop All"
    /// The note shown when the instances changed since the preview and a new one was read.
    public static let changed = "The list changed. Review it before you stop."

    /// The secondary text of an item: where its demand comes from, joined by ", ".
    /// - Parameter demand: The sources of the instance.
    public static func demand(_ demand: [DemandSourceKind]) -> String {
        demand.map { source in
            switch source {
            case .manual: "Started by you"
            case .startup: "Started at login"
            default: "Used by an environment"
            }
        }
        .joined(separator: ", ")
    }

    /// The secondary text of an item: ``demand(_:)``, with "Used by {environment}" (names
    /// joined by ", ") for the environment source when the preview names the environments
    /// (spec 0047, *Copy Stop All*).
    /// - Parameter item: The item of the preview.
    public static func demand(of item: StopAllItem) -> String {
        let environments = (item.environments ?? []).map(\.name)
        guard environments.isEmpty == false else { return demand(item.demand) }
        return item.demand.map { source in
            source == .environment
                ? "Used by \(environments.joined(separator: ", "))" : demand([source])
        }
        .joined(separator: ", ")
    }

    /// The line about the environments Stop All leaves incomplete, or `nil` when it disturbs
    /// none (spec 0047, *Copy Stop All*): "{environment} will no longer be complete. Its
    /// lease stays." or "{n} environments will no longer be complete: {names}. Their leases
    /// stay."
    public static func affected(_ preview: StopAllPreview) -> String? {
        let names = (preview.affectedEnvironments ?? []).map(\.name)
        switch names.count {
        case 0: return nil
        case 1: return "\(names[0]) will no longer be complete. Its lease stays."
        default:
            return
                "\(names.count) environments will no longer be complete: \(names.joined(separator: ", ")). Their leases stay."
        }
    }

    /// The row of an instance that is stopping on its own.
    public static func alreadyStopping(_ name: String) -> String {
        "\(name) · already stopping"
    }

    /// The row of an instance Stop All leaves alone.
    public static func skipped(_ name: String, code: String) -> String {
        "\(name) · won’t stop: \(reason(of: code))"
    }

    /// The sentence for the code an instance is left alone with.
    public static func reason(of code: String) -> String {
        switch code {
        case "supervisor.identity_unproven": "Stackly can’t confirm this process"
        case "supervisor.stop_requires_override": "used by an environment"
        case "operation.recovery_required":
            "an operation for it needs your attention in Stackly"
        default: code
        }
    }

    /// The last line of the preview: how many of the instances start again at the next login.
    /// - Parameter atLoginCount: ``StopAllPreview/atLoginCount``.
    public static func atLogin(_ atLoginCount: UInt64) -> String {
        switch atLoginCount {
        case 0: "None of them start at your next login."
        case 1: "1 of them starts again at your next login."
        default: "\(atLoginCount) of them start again at your next login."
        }
    }

    /// The footer of the preview: ``dataKept`` and the sentence for the next login.
    public static func footer(_ preview: StopAllPreview) -> String {
        "\(dataKept) \(atLogin(preview.atLoginCount))"
    }

    /// What a finished Stop All says.
    ///
    /// - Parameters:
    ///   - result: The answer of `instance.stop_all`.
    ///   - preview: The preview the person confirmed, for the total and the names.
    /// - Returns: The sentence, and whether it comes with Open Stackly.
    public static func outcome(of result: StopAllResult, preview: StopAllPreview)
        -> (text: String, opensStackly: Bool)
    {
        let total = preview.items.count
        let stopped = result.stopped.count
        if result.skipped.contains(where: { $0.code == "operation.cancelled" }) {
            return ("Stop All was cancelled. Stopped \(stopped) of \(total).", true)
        }
        let names = Dictionary(
            preview.items.map { ($0.instanceId, $0.name) }, uniquingKeysWith: { first, _ in first })
        let failedNames = result.failed.map { names[$0.instanceId] ?? "an instance" }
        var text: String
        switch failedNames.count {
        case 0:
            text = "Stopped \(stopped) \(stopped == 1 ? "service" : "services"). \(dataKept)"
        case 1:
            text = "Stopped \(stopped) of \(total). \(failedNames[0]) didn’t stop."
        default:
            text =
                "Stopped \(stopped) of \(total). \(failedNames.count) didn’t stop: "
                + "\(failedNames.joined(separator: ", "))."
        }
        if result.skipped.isEmpty == false {
            text += " \(result.skipped.count) skipped."
        }
        return (text, failedNames.isEmpty == false)
    }
}
