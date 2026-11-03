import Foundation
import StacklyClient

/// The words and identifiers of the four notifications (spec 0032, *Pemicu notifikasi*). They
/// are pure functions of what the daemon sent, and none holds a secret, a path, or a line of a
/// log.
nonisolated enum NotificationContent {
    /// The operation kinds that notify, the ones the popover lists (spec 0032, *Operasi di
    /// popover*).
    static let operationKinds: Set<OperationKind> = [
        .installRuntime, .createInstance, .startInstance, .stopInstance, .restartInstance,
        .stopAllInstances,
    ]

    /// An operation notifies only when it ran longer than this.
    static let longOperation: TimeInterval = 10

    /// An instance notifies about lost readiness at most once in this time.
    static let unreadyInterval: TimeInterval = 5 * 60

    /// How long the daemon must stay out of reach before one notification.
    static let unreachableDelay: Duration = .seconds(15)

    /// An instance failed.
    static func failed(_ instance: InstanceSummary) -> AppNotification {
        AppNotification(
            identifier:
                "instance-failed-\(instance.id.uuidString.lowercased())-\(instance.revision)",
            title: "\(instance.name) failed", body: "Open Stackly to see what happened.",
            target: .instance(instance.id))
    }

    /// A running instance stopped accepting connections.
    static func unready(name: String, id: UUID, statusSeq: UInt64) -> AppNotification {
        AppNotification(
            identifier: "instance-unready-\(id.uuidString.lowercased())-\(statusSeq)",
            title: "\(name) isn’t ready", body: "It stopped accepting connections.",
            target: .instance(id))
    }

    /// A long operation ended.
    /// - Parameters:
    ///   - operation: The operation, in a state that has ended.
    ///   - title: The title of the operation, from ``OperationTitle``.
    ///   - duration: How long it ran.
    static func operation(
        _ operation: OperationSummary, title: String, duration: TimeInterval
    ) -> AppNotification? {
        let heading: String
        let body: String
        switch operation.state {
        case .succeeded:
            heading = "\(title) finished"
            body = "Took \(durationText(duration))."
        case .failed:
            heading = "\(title) failed"
            body = "Open Stackly to see what happened."
        case .cancelled:
            heading = "\(title) was cancelled"
            body = "It was cancelled after \(durationText(duration))."
        default:
            return nil
        }
        return AppNotification(
            identifier: "operation-\(operation.id.uuidString.lowercased())", title: heading,
            body: body, target: .operations)
    }

    /// The daemon stayed out of reach.
    /// - Parameter since: When it went out of reach.
    static func unreachable(since: Date) -> AppNotification {
        AppNotification(
            identifier: "daemon-unreachable-\(Int(since.timeIntervalSince1970))",
            title: "stacklyd isn’t reachable",
            body: "Your instances keep running if stacklyd comes back.", target: .diagnostics)
    }

    /// "12 seconds" or "3 minutes".
    static func durationText(_ seconds: TimeInterval) -> String {
        let whole = Int(seconds.rounded())
        if whole < 60 { return whole == 1 ? "1 second" : "\(whole) seconds" }
        let minutes = whole / 60
        return minutes == 1 ? "1 minute" : "\(minutes) minutes"
    }
}
