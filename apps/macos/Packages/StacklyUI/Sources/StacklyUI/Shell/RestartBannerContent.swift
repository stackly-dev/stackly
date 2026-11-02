import Foundation
import StacklyClient

/// The one banner the main window shows for a daemon restart (spec 0030 AC-16): "stacklyd is
/// restarting" while the daemon is away, the summary while the new daemon brings the
/// instances back, and a warning when the restart did not go to plan.
///
/// A pure value made from the plan the daemon reported, so every state can be tested and
/// drawn without a daemon. A restart that finished without a failure shows no banner at all;
/// its result is the toast (``DaemonRestartModel/toast``).
nonisolated public struct RestartBannerContent: Equatable, Sendable {
    /// One instance the banner lists.
    nonisolated public struct Item: Equatable, Sendable {
        /// The instance name when the plan was written.
        public let name: String
        /// What became of it.
        public let reason: String
    }

    /// How long a plan that ended stays in the banner, the same 10 minutes the daemon keeps
    /// it in `DaemonInfo.restart` (`RESTART_SUMMARY_TTL`, spec 0030).
    public static let summaryTTL: TimeInterval = 600

    /// The tone: informative while it runs, caution when it did not go to plan.
    public let tone: Tone
    /// The title.
    public let title: String
    /// The message under the title.
    public let message: String
    /// The instances the banner lists, empty for most states.
    public let items: [Item]
    /// Whether the person can dismiss the banner, which is only so once the plan ended.
    public let canDismiss: Bool

    /// What the instances of a plan came to.
    nonisolated public struct Counts: Equatable, Sendable {
        /// Instances the new daemon adopted.
        public var adopted = 0
        /// Instances it started again.
        public var restored = 0
        /// Instances that did not end up running.
        public var failed = 0
        /// Instances that were removed, lost their data, or never got their turn.
        public var skipped = 0
        /// Instances without an outcome yet.
        public var pending = 0
        /// Instances the restart keeps running.
        public var keeping = 0
        /// Instances the restart stops and starts again.
        public var stopping = 0
    }

    /// The tally of `plan`'s instances.
    public static func counts(of plan: RestartPlanView) -> Counts {
        var counts = Counts()
        for instance in plan.instances ?? [] {
            switch instance.decision {
            case .keep: counts.keeping += 1
            case .stopRestore: counts.stopping += 1
            case .unrecognized: break
            }
            switch instance.outcome {
            case .adopted: counts.adopted += 1
            case .restored: counts.restored += 1
            case .failed: counts.failed += 1
            case .skipped: counts.skipped += 1
            case .unrecognized: break
            case nil: counts.pending += 1
            }
        }
        return counts
    }

    /// The banner for a plan.
    /// - Parameters:
    ///   - plan: The plan the daemon last reported, or `nil`.
    ///   - isAwaitingPlan: Whether the person asked for a restart and the plan has not
    ///     arrived.
    ///   - dismissedPlanID: The plan whose banner the person dismissed.
    ///   - now: The time now.
    /// - Returns: The banner, or `nil` when there is nothing to show.
    public static func make(
        plan: RestartPlanView?, isAwaitingPlan: Bool, dismissedPlanID: UUID?, now: Date
    ) -> RestartBannerContent? {
        guard let plan else {
            return isAwaitingPlan
                ? RestartBannerContent(
                    tone: .informative, title: RestartCopy.restartingTitle,
                    message: "stacklyd is checking which instances can carry on.", items: [],
                    canDismiss: false)
                : nil
        }
        let counts = counts(of: plan)
        switch plan.state {
        case .stopping, .handedOver:
            return RestartBannerContent(
                tone: .informative, title: RestartCopy.restartingTitle,
                message: awayMessage(counts), items: [], canDismiss: false)
        case .restoring:
            return RestartBannerContent(
                tone: .informative, title: "stacklyd restarted",
                message: summary(plan, counts), items: [], canDismiss: false)
        case .finished:
            guard counts.failed > 0, isFresh(plan, now: now), plan.id != dismissedPlanID else {
                return nil
            }
            let word = counts.failed == 1 ? "instance" : "instances"
            return RestartBannerContent(
                tone: .caution,
                title: "stacklyd restarted, but \(counts.failed) \(word) did not come back",
                message: summary(plan, counts),
                items: (plan.instances ?? []).filter { $0.outcome == .failed }.map {
                    Item(name: $0.name, reason: RestartCopy.failure($0.failureCode))
                },
                canDismiss: true)
        case .aborted:
            guard isFresh(plan, now: now), plan.id != dismissedPlanID else { return nil }
            return RestartBannerContent(
                tone: .caution, title: "Daemon restart aborted",
                message:
                    "\(RestartCopy.closeReason(plan.closeReason)) stacklyd is still running. The instances it stopped are being started again, so check their state in Instances.",
                items: abortedItems(of: plan),
                canDismiss: true)
        case .expired:
            guard isFresh(plan, now: now), plan.id != dismissedPlanID else { return nil }
            return RestartBannerContent(
                tone: .caution, title: "Daemon restart expired",
                message:
                    "\(RestartCopy.closeReason(plan.closeReason)) The instances it stopped stay stopped.",
                items: [], canDismiss: true)
        case .unrecognized:
            return nil
        }
    }

    /// The two lists of an aborted plan, one after the other, split by the `stoppedAt` of the
    /// `stop_restore` rows (spec 0030, *Value sourcing*): the instances that were stopped and
    /// are being started again, then the ones that did not stop and still run.
    private static func abortedItems(of plan: RestartPlanView) -> [Item] {
        let stopRows = (plan.instances ?? []).filter { $0.decision == .stopRestore }
        let stopped = stopRows.filter { $0.stoppedAt != nil }.map {
            Item(name: $0.name, reason: RestartCopy.stoppedStartingAgain)
        }
        let running = stopRows.filter { $0.stoppedAt == nil }.map {
            Item(name: $0.name, reason: RestartCopy.didNotStopStillRunning)
        }
        return stopped + running
    }

    private static func isFresh(_ plan: RestartPlanView, now: Date) -> Bool {
        now.timeIntervalSince(plan.finishedAt ?? plan.createdAt) < summaryTTL
    }

    /// "3 instances keep running. 1 instance stops and starts again."
    private static func awayMessage(_ counts: Counts) -> String {
        let stops =
            counts.stopping == 1
            ? "\(RestartCopy.instances(1)) stops and starts again."
            : "\(RestartCopy.instances(counts.stopping)) stop and start again."
        let keeps = "\(RestartCopy.instances(counts.keeping)) keep running."
        switch (counts.keeping, counts.stopping) {
        case (0, 0): return "No instance is affected."
        case (_, 0): return keeps
        case (0, _): return stops
        default: return "\(keeps) \(stops)"
        }
    }

    /// The summary: what came of the instances so far and how long each part took.
    private static func summary(_ plan: RestartPlanView, _ counts: Counts) -> String {
        var parts = ["\(counts.adopted) adopted", "\(counts.restored) started again"]
        parts.append("\(counts.failed) failed")
        if counts.pending > 0 { parts.append("\(counts.pending) still starting") }
        var text = parts.joined(separator: ", ") + "."
        var spans: [String] = []
        if let handedOver = plan.handedOverAt {
            spans.append(
                "stopping \(RestartCopy.duration(handedOver.timeIntervalSince(plan.createdAt)))")
            if let reconciled = plan.reconciledAt {
                spans.append(
                    "daemon away \(RestartCopy.duration(reconciled.timeIntervalSince(handedOver)))")
                if let finished = plan.finishedAt {
                    spans.append(
                        "restoring \(RestartCopy.duration(finished.timeIntervalSince(reconciled)))")
                }
            }
        }
        if spans.isEmpty == false {
            let joined = spans.joined(separator: ", ")
            text += " \(joined.prefix(1).uppercased())\(joined.dropFirst())."
        }
        return text
    }
}
