import Foundation
import StacklyClient

/// Restart, Reload, and Force Stop of an instance: whether each can run and why not (spec
/// 0026, *Alasan nonaktif*). A pure function of the summary and the local status, next to
/// ``InstanceAction/resolve(summary:local:)``, so the toolbar, the Instance menu, and the
/// fixtures agree. A daemon that is offline is the mutation gate's reason, which every action
/// that mutates already reads, so it is not repeated here.
nonisolated public struct InstanceLifecycleActions: Equatable, Sendable {
    /// One of the three actions.
    public struct Item: Equatable, Sendable {
        /// The verb a click sends.
        public let verb: InstanceVerb
        /// The title: the busy label while its own click runs.
        public let title: String
        /// The SF Symbol.
        public let symbol: String
        /// Why it cannot run, or `nil`.
        public let disabledReason: String?
        /// Whether a click of this window runs for the instance.
        public let isBusy: Bool
    }

    /// Restart.
    public let restart: Item
    /// Reload.
    public let reload: Item
    /// Force Stop….
    public let forceStop: Item

    /// The reason an `unknown` instance gives.
    static let unproven = "Stackly can’t confirm this instance’s process."

    /// The three actions of `summary`.
    /// - Parameters:
    ///   - summary: The instance.
    ///   - local: What this window knows about its own click.
    ///   - providerName: The name to show for the provider, such as "NATS".
    public static func resolve(
        summary: InstanceSummary, local: InstanceLocalStatus, providerName: String
    ) -> InstanceLifecycleActions {
        let lifecycle = summary.lifecycle
        func item(_ verb: InstanceVerb, title: String, symbol: String, reason: String?) -> Item {
            if let busy = local.verb {
                return Item(
                    verb: verb, title: busy == verb ? "\(verb.progressive)…" : title,
                    symbol: symbol, disabledReason: nil, isBusy: true)
            }
            return Item(
                verb: verb, title: title, symbol: symbol, disabledReason: reason, isBusy: false)
        }
        let restartReason: String? =
            switch lifecycle {
            case .unknown: unproven
            case .running, .recovering: nil
            default: "Restart needs a running instance."
            }
        var reloadReason: String?
        if summary.capabilities?.reload != true {
            reloadReason =
                "\(providerName) doesn’t support reload. Restart to apply configuration changes."
        } else if lifecycle == .unknown {
            reloadReason = unproven
        } else if lifecycle != .running {
            reloadReason = "Reload needs a running instance."
        }
        let forceReason: String? =
            switch lifecycle {
            case .unknown: unproven
            case .stopping: nil
            default: "Force stop is available while an instance is still stopping."
            }
        return InstanceLifecycleActions(
            restart: item(
                .restart, title: "Restart", symbol: "arrow.clockwise", reason: restartReason),
            reload: item(.reload, title: "Reload", symbol: "bolt", reason: reloadReason),
            forceStop: item(
                .forceStop, title: "Force Stop…", symbol: "xmark.octagon", reason: forceReason))
    }

    /// The help of an enabled Restart or Reload button (spec 0026, *Copy*).
    static func help(of item: Item) -> String {
        if let reason = item.disabledReason { return reason }
        if item.isBusy { return item.title }
        return item.verb == .reload ? "Reload configuration" : item.title
    }
}
