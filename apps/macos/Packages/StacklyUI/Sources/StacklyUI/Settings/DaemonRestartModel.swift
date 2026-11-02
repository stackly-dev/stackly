import Foundation
import Observation
import StacklyClient

/// How a restart request ended, by what the daemon may have done (the shape of
/// ``MutationOutcome`` for the one call that needs the long time limit).
public enum RestartOutcome: Equatable, Sendable {
    /// The daemon wrote the plan and will stop.
    case accepted(ShutdownAccepted)
    /// The daemon refused it; nothing changed.
    case rejected(ErrorBody)
    /// Nothing reached the daemon.
    case notSent(ErrorBody)
    /// The request was written but no answer came; a retry reuses the key.
    case outcomeUnknown(ErrorBody)
}

/// The state behind Restart Daemon: the confirmation sheet, the plan the daemon last
/// reported, and the banner, row marks, and toast that follow it (spec 0030 AC-16).
///
/// The daemon model feeds it from `DaemonInfo.restart` after every handshake and from the
/// `daemon.restart_plan` events, so a window that opens in the middle of a restart, or after
/// one, shows the same thing as a window that watched it. Every decision about what to show
/// is a pure function of ``plan`` (see ``RestartBannerContent``), and the model only keeps
/// what cannot be derived: the sheet, the plan, the toast that waits to be shown, and the
/// banner the person dismissed.
@Observable
public final class DaemonRestartModel {
    /// What the confirmation sheet shows.
    public struct Confirmation: Equatable {
        /// The plan the person is asked about.
        public var preview: RestartPreview
        /// Whether the restart request is out, so the sheet shows its progress.
        public var isWorking = false
        /// The reason the last request failed, or `nil`.
        public var error: ErrorBody?
        /// Whether the plan changed under the person since the sheet opened.
        public var planChanged = false
    }

    /// The sheet over the Settings window, or `nil` while it is closed.
    public private(set) var confirmation: Confirmation?
    /// Whether the preview is being read, so the button shows progress.
    public private(set) var isLoadingPreview = false
    /// Why the preview could not be read, or `nil`.
    public private(set) var previewError: ErrorBody?
    /// The plan the daemon last reported, or `nil` before the first restart.
    public private(set) var plan: RestartPlanView?
    /// Whether this window asked for a restart whose plan has not arrived yet.
    public private(set) var isAwaitingPlan = false
    /// The toast that waits to be shown once, or `nil`.
    public private(set) var toast: Toast?
    /// The plan whose banner the person dismissed.
    public private(set) var dismissedPlanID: UUID?

    /// A toast: its own identity tells one from the next.
    public struct Toast: Equatable, Identifiable {
        /// Tells one toast from another.
        public let id: UUID
        /// The sentence.
        public let message: String
    }

    @ObservationIgnored private let preview: () async throws(ClientError) -> RestartPreview
    @ObservationIgnored private let restart: (String, Bool, String) async -> RestartOutcome
    @ObservationIgnored private let now: () -> Date
    @ObservationIgnored private var retryKey: (digest: String, key: String)?
    @ObservationIgnored private var watchedPlan: UUID?

    /// Creates a model.
    /// - Parameters:
    ///   - preview: Reads `daemon.restart_preview`.
    ///   - restart: Sends `daemon.shutdown { restart }` with the confirmed digest, the force
    ///     flag, and the idempotency key.
    ///   - now: The wall clock.
    public init(
        preview: @escaping () async throws(ClientError) -> RestartPreview,
        restart: @escaping (String, Bool, String) async -> RestartOutcome,
        now: @escaping () -> Date = { Date() }
    ) {
        self.preview = preview
        self.restart = restart
        self.now = now
    }

    // MARK: - From the daemon

    /// Takes the plan of `DaemonInfo.restart` after a handshake, or after the stores were
    /// loaded again. A daemon that reports none changes nothing: the new daemon opens its
    /// socket before it takes the plan over, so the first answer is empty while the restart
    /// goes on. A plan nobody takes over ends with the reconciliation (see
    /// ``receive(event:)``).
    /// - Parameter info: The reply of `daemon.get_info`, or `nil` when it did not answer.
    public func receive(info: DaemonInfo?) {
        guard let view = info?.restart else { return }
        accept(view)
    }

    /// Takes one event: a `daemon.restart_plan`, or the end of a reconciliation, which closes
    /// a plan that the new daemon did not take over.
    public func receive(event: Event) {
        switch event.kind {
        case .daemonRestartPlan(let view):
            accept(view)
        case .daemonReconciling(let reconciling) where reconciling.phase == .finished:
            dropPlanNobodyTook()
        default:
            break
        }
    }

    /// A reconciliation that ended leaves no plan the old daemon wrote and the new one did
    /// not take over: such a plan is still `stopping` or `handed_over`, and the banner would
    /// wait for an event that never comes. The plan stays watched, so a late event for it
    /// still makes the toast.
    private func dropPlanNobodyTook() {
        if let plan, plan.state == .stopping || plan.state == .handedOver {
            self.plan = nil
        }
        isAwaitingPlan = false
    }

    private func accept(_ view: RestartPlanView) {
        if let current = plan, current.id == view.id, view.state.rank < current.state.rank {
            return
        }
        let watching = isAwaitingPlan || watchedPlan == view.id
        plan = view
        isAwaitingPlan = false
        if view.state.isOpen {
            watchedPlan = view.id
        } else if watching, view.state == .finished {
            watchedPlan = nil
            let counts = RestartBannerContent.counts(of: view)
            toast = Toast(
                id: UUID(),
                message: RestartCopy.toast(adopted: counts.adopted, restored: counts.restored))
        } else if watching {
            watchedPlan = nil
        }
    }

    /// Marks the toast as shown.
    public func consumeToast(_ id: UUID) {
        if toast?.id == id { toast = nil }
    }

    /// Hides the banner of the plan that ended.
    public func dismissBanner() {
        dismissedPlanID = plan?.id
    }

    // MARK: - Derived

    /// The banner for the plan now, or `nil` when there is none to show.
    /// - Parameters:
    ///   - now: The time now.
    ///   - daemonBannerShown: Whether the daemon has a banner of its own (it is down, or
    ///     degraded). A restart that is running replaces it; one that ended gives way.
    public func banner(now: Date, daemonBannerShown: Bool = false) -> RestartBannerContent? {
        guard
            let content = RestartBannerContent.make(
                plan: plan, isAwaitingPlan: isAwaitingPlan, dismissedPlanID: dismissedPlanID,
                now: now)
        else { return nil }
        return isRestarting || daemonBannerShown == false ? content : nil
    }

    /// The instances the restart stopped and has not brought back, by ID, for the rows of
    /// the Instances page (the mark is ``RestartCopy/rowMark``).
    public var markedInstances: Set<UUID> {
        guard let plan, plan.state.isOpen else { return [] }
        return Set(
            (plan.instances ?? [])
                .filter { $0.decision == .stopRestore && $0.stoppedAt != nil && $0.outcome == nil }
                .map(\.instanceId))
    }

    /// Whether a restart runs now, so the button stays off.
    public var isRestarting: Bool {
        isAwaitingPlan || plan?.state.isOpen == true
    }

    // MARK: - The sheet

    /// Reads the plan and opens the sheet over it.
    public func begin() async {
        guard confirmation == nil, isLoadingPreview == false else { return }
        isLoadingPreview = true
        previewError = nil
        defer { isLoadingPreview = false }
        do {
            confirmation = Confirmation(preview: try await preview())
        } catch {
            previewError = Self.body(of: error)
        }
    }

    /// Closes the sheet without restarting.
    public func cancel() {
        guard confirmation?.isWorking != true else { return }
        confirmation = nil
    }

    /// Reads the plan again inside the open sheet, after the daemon refused the old one.
    public func reload(planChanged: Bool = false) async {
        guard var current = confirmation else { return }
        do {
            current.preview = try await preview()
            current.planChanged = planChanged
            current.isWorking = false
            confirmation = current
        } catch {
            current.error = Self.body(of: error)
            current.isWorking = false
            confirmation = current
        }
    }

    /// Asks the daemon to restart with the plan the sheet shows.
    /// - Parameter force: Restart although an operation that cannot resume still runs.
    public func confirm(force: Bool = false) async {
        guard var current = confirmation, current.isWorking == false else { return }
        let digest = current.preview.digest
        // A retry after a lost answer is the same action, so it reuses the key.
        let key: String
        if let retryKey, retryKey.digest == digest {
            key = retryKey.key
        } else {
            key = MutationPath.newKey()
        }
        current.isWorking = true
        current.error = nil
        current.planChanged = false
        confirmation = current
        switch await restart(digest, force, key) {
        case .accepted:
            retryKey = nil
            isAwaitingPlan = plan?.state.isOpen != true
            dismissedPlanID = nil
            confirmation = nil
        case .rejected(let body):
            retryKey = nil
            if body.code == "daemon.restart_plan_changed" {
                await reload(planChanged: true)
            } else if body.code == "daemon.restart_busy" {
                await reload()
                confirmation?.error = body
            } else {
                finishWorking(with: body)
            }
        case .notSent(let body):
            retryKey = nil
            finishWorking(with: body)
        case .outcomeUnknown(let body):
            retryKey = (digest, key)
            finishWorking(with: body)
        }
    }

    private func finishWorking(with body: ErrorBody) {
        confirmation?.isWorking = false
        confirmation?.error = body
    }

    private static func body(of error: ClientError) -> ErrorBody {
        switch error {
        case .rejected(let body), .notSent(let body), .outcomeUnknown(let body): body
        }
    }
}

extension PlanState {
    /// Whether the plan is not final.
    nonisolated var isOpen: Bool {
        switch self {
        case .stopping, .handedOver, .restoring: true
        case .finished, .aborted, .expired, .unrecognized: false
        }
    }

    /// The place in the order of the state machine, so a late event never moves a plan back.
    nonisolated var rank: Int {
        switch self {
        case .stopping: 0
        case .handedOver: 1
        case .restoring: 2
        case .finished, .aborted, .expired, .unrecognized: 3
        }
    }
}
