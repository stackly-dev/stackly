import Foundation
import StacklyClient

/// Every user facing string of Down (spec 0047, *Copy Down*). The spec wins over the mockup
/// where they differ.
extension EnvironmentCopy {
    static let down = "Down"
    static let bringDown = "Bring Down"
    static let bringDownAgain = "Bring Down…"
    static let alreadyBringingDown = "Already bringing down"
    static let nothingToBringDown = "Nothing to bring down."
    static let downChanged = "What Down will stop changed. Review it again."
    static let downFirstLine =
        "Releases the lease and stops only instances nothing else needs. Shared and manually started instances keep running. Data is kept."
    static let downNothingToStop = "Nothing needs to stop."
    static let downCancelsUp = "This also cancels the Up in progress."
    /// The detail of a Down that kept instances running, from the confirmation's first line,
    /// so an environment that still reads partially ready is explained.
    static let downKeptDetail = "Shared and manually started instances keep running."
    /// The most names a line of the confirmation lists before "and {n} more".
    static let downNameLimit = 6

    /// "Bring down “{name}”?"
    static func bringDownTitle(_ name: String) -> String {
        "Bring down “\(name)”?"
    }

    /// "Bringing down… {done} of {total}", or "Bringing down…" without a count.
    static func bringingDown(done: UInt64?, total: UInt64?) -> String {
        guard let done, let total, total > 0 else { return "Bringing down…" }
        return "Bringing down… \(done) of \(total)"
    }

    /// The message of the Down confirmation, one line per part: what Down does, what stops,
    /// what keeps running and why, what can't be stopped, and the Up it cancels. `notice`
    /// comes first after `env.down_changed`.
    static func downMessage(_ preview: EnvDownPreview, notice: String? = nil) -> String {
        var lines: [String] = []
        if let notice { lines.append(notice) }
        lines.append(downFirstLine)
        if preview.stop.isEmpty {
            lines.append(downNothingToStop)
        } else {
            lines.append("Stops: \(names(preview.stop.map(\.name))).")
        }
        if let keep = keepLine(preview.keep) { lines.append(keep) }
        if preview.blocked.isEmpty == false {
            lines.append(
                "Can’t stop: \(names(preview.blocked.map { "\($0.instance.name) (\(blockedReason($0.code)))" }))."
            )
        }
        if preview.cancelsUp { lines.append(downCancelsUp) }
        return lines.joined(separator: "\n")
    }

    /// "Keeps running: {name} ({reason}), …", naming a shared instance with the environments
    /// that use it and one you started with "started by you", or `nil` when none keeps
    /// running. The confirmation and the result of a Down both write it.
    static func keepLine(_ keep: [DownKeep]) -> String? {
        guard keep.isEmpty == false else { return nil }
        return "Keeps running: \(names(keep.map { "\($0.instance.name) (\(keepReason($0)))" }))."
    }

    /// At most ``downNameLimit`` names joined with ", ", then " and {n} more".
    static func names(_ names: [String]) -> String {
        guard names.count > downNameLimit else { return names.joined(separator: ", ") }
        let shown = names.prefix(downNameLimit).joined(separator: ", ")
        return "\(shown) and \(names.count - downNameLimit) more"
    }

    /// Why an instance keeps running: "used by {environment}", "started by you", "starts at
    /// login", joined with " and ".
    static func keepReason(_ keep: DownKeep) -> String {
        var reasons: [String] = []
        for source in keep.demand {
            switch source {
            case .environment:
                let environments = (keep.environments ?? []).map(\.name)
                let reason =
                    environments.isEmpty
                    ? "used by an environment"
                    : "used by \(environments.joined(separator: ", "))"
                if reasons.contains(reason) == false { reasons.append(reason) }
            case .manual:
                if reasons.contains("started by you") == false { reasons.append("started by you") }
            case .startup:
                if reasons.contains("starts at login") == false {
                    reasons.append("starts at login")
                }
            case .unrecognized:
                continue
            }
        }
        return reasons.isEmpty ? "still needed" : reasons.joined(separator: " and ")
    }

    /// Why Down can't stop an instance, by its code, or the code itself.
    static func blockedReason(_ code: String) -> String {
        switch code {
        case "supervisor.identity_unproven": "Stackly can’t confirm this process"
        case "operation.recovery_required": "an operation for it needs your attention"
        default: code
        }
    }

    // MARK: - Feedback

    /// "{name} is down. {s} instances stopped. Data is kept."
    static func downStopped(_ name: String, stopped: Int) -> String {
        let count = stopped == 1 ? "1 instance" : "\(stopped) instances"
        return "\(name) is down. \(count) stopped. Data is kept."
    }

    /// "{name} is down. {s} stopped, {k} kept running."
    static func downKept(_ name: String, stopped: Int, kept: Int) -> String {
        "\(name) is down. \(stopped) stopped, \(kept) kept running."
    }

    /// "{name} released its lease. Nothing needed to stop."
    static func downReleased(_ name: String) -> String {
        "\(name) released its lease. Nothing needed to stop."
    }

    /// "{name} is not fully down. {instance} didn’t stop." or "{f} instances didn’t stop."
    static func downIncomplete(_ name: String, instances: [String], count: Int) -> String {
        if count <= 1, let instance = instances.first {
            return "\(name) is not fully down. \(instance) didn’t stop."
        }
        return "\(name) is not fully down. \(count) instances didn’t stop."
    }

    /// "{name} released its lease, but {instance} is still running. Bring it down again."
    static func downLeftRunning(_ name: String, instances: [String]) -> String {
        let list =
            instances.count > 1
            ? "\(instances.dropLast().joined(separator: ", ")) and \(instances[instances.count - 1]) are"
            : "\(instances.first ?? "an instance") is"
        return "\(name) released its lease, but \(list) still running. Bring it down again."
    }

    /// "Bringing down {name} was cancelled. Its lease is released."
    static func downCancelledAfterRelease(_ name: String) -> String {
        "Bringing down \(name) was cancelled. Its lease is released."
    }

    /// "Bringing down {name} was cancelled before it started."
    static func downCancelledBeforeStart(_ name: String) -> String {
        "Bringing down \(name) was cancelled before it started."
    }
}
