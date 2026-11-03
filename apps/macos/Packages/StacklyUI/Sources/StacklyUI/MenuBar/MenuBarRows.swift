import Foundation
import StacklyClient

/// The button at the end of a Services row (spec 0032, *Tombol baris*).
nonisolated public struct MenuBarRowButton: Equatable, Sendable {
    /// Which verb the button stands for.
    public enum Kind: Equatable, Sendable {
        /// Starts the instance.
        case start
        /// Stops the instance.
        case stop
    }

    /// Start or Stop.
    public let kind: Kind
    /// Whether a click sends the request.
    public let isEnabled: Bool
    /// The tooltip: the reason when the button is off, else the action.
    public let tooltip: String

    /// The verb a click sends.
    public var verb: InstanceVerb { kind == .start ? .start : .stop }
}

/// The rules of the Services section: which instances show, in which order, and what each
/// row's button does (spec 0032, AC-3 and AC-4). Pure functions, so the table is tested
/// without a view.
nonisolated public enum MenuBarRows {
    /// The most rows the section shows.
    public static let limit = 8

    /// The group of a lifecycle in the order of the section, or `nil` for a retained
    /// instance, which no list shows: (1) failed; (2) starting, stopping, recovering,
    /// unknown; (3) running; (4) stopped, uninitialized, initializing.
    static func rank(of lifecycle: InstanceLifecycle) -> Int? {
        switch lifecycle {
        case .failed: 0
        case .starting, .stopping, .recovering, .unknown: 1
        case .running: 2
        case .stopped, .uninitialized, .initializing: 3
        case .retained: nil
        }
    }

    /// The instances in the order of the section: by group, then by name with
    /// `localizedStandardCompare`, then by ID when the names are equal.
    public static func ordered(_ instances: [InstanceSummary]) -> [InstanceSummary] {
        instances
            .compactMap { instance in rank(of: instance.lifecycle).map { ($0, instance) } }
            .sorted { first, second in
                if first.0 != second.0 { return first.0 < second.0 }
                switch first.1.name.localizedStandardCompare(second.1.name) {
                case .orderedAscending: return true
                case .orderedDescending: return false
                case .orderedSame: return first.1.id.uuidString < second.1.id.uuidString
                }
            }
            .map(\.1)
    }

    /// The button of an instance: the first row of the table that fits.
    /// - Parameters:
    ///   - summary: The instance.
    ///   - isConnected: Whether the presence is `connected`.
    ///   - hasOtherOperation: Whether an operation of this instance runs, or this window's
    ///     own click for it is on its way.
    public static func button(
        for summary: InstanceSummary, isConnected: Bool, hasOtherOperation: Bool
    ) -> MenuBarRowButton {
        let name = summary.name
        let kind: MenuBarRowButton.Kind
        switch summary.lifecycle {
        case .starting, .running, .recovering, .stopping, .unknown: kind = .stop
        default: kind = .start
        }
        func off(_ reason: String) -> MenuBarRowButton {
            MenuBarRowButton(kind: kind, isEnabled: false, tooltip: reason)
        }
        if isConnected == false { return off("stacklyd isn’t reachable.") }
        if hasOtherOperation { return off("Another operation is running for this instance.") }
        switch summary.lifecycle {
        case .uninitialized, .initializing: return off("Still being created.")
        case .unknown:
            return off("Stackly can’t confirm this process. Open Stackly to check it.")
        case .stopping: return off("Stopping…")
        // provisional: the contract has no start or stop capability, only `reload`, so the
        // row "{provider} doesn’t support this." of the table cannot happen yet.
        case .stopped, .failed:
            return MenuBarRowButton(kind: .start, isEnabled: true, tooltip: "Start \(name)")
        case .starting, .running, .recovering:
            return MenuBarRowButton(kind: .stop, isEnabled: true, tooltip: "Stop \(name)")
        case .retained: return off("This instance was removed.")
        }
    }
}

/// The variant of the menu bar icon (spec 0032, AC-1): offline wins over failed, failed over
/// normal.
nonisolated public enum MenuBarIconVariant: Equatable, Sendable, CaseIterable {
    /// Connected and nothing failed.
    case normal
    /// Connected and at least one instance failed: a dot is part of the shape.
    case failed
    /// The daemon cannot be reached: a slash is part of the shape.
    case offline

    /// The variant for a presence and the number of failed instances.
    /// - Parameters:
    ///   - isConnected: Whether the presence is `connected`.
    ///   - failedCount: How many instances have the lifecycle `failed` in the last state.
    public static func of(isConnected: Bool, failedCount: Int) -> MenuBarIconVariant {
        if isConnected == false { return .offline }
        return failedCount > 0 ? .failed : .normal
    }

    /// The name of the icon, for the label of the menu bar item.
    public var title: String {
        switch self {
        case .normal: "Stackly"
        case .failed: "Stackly, an instance failed"
        case .offline: "Stackly, stacklyd isn’t reachable"
        }
    }
}
