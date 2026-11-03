import Foundation
import Observation
import StacklyClient

/// How an `agent_off` request ended, by what the daemon may have done.
public enum TurnOffOutcome: Equatable, Sendable {
    /// The daemon answered.
    case accepted(ShutdownAccepted)
    /// The daemon refused it; nothing changed.
    case rejected(ErrorBody)
    /// Nothing reached the daemon.
    case notSent(ErrorBody)
    /// The request was written but no answer came; a retry reuses the key.
    case outcomeUnknown(ErrorBody)
}

/// Turn Off… in Diagnostics (spec 0031, AC-19 and AC-20): reads the preview, shows the plan
/// for the confirmation, asks the daemon to stop its instances, and unregisters the agent
/// only when the daemon says it exits. A daemon that stays up leaves the agent registered and
/// the sentence names what could not be stopped.
@Observable
public final class TurnOffModel {
    /// What the model is doing.
    public enum Phase: Equatable {
        /// Nothing runs.
        case idle
        /// The preview is being read.
        case loadingPreview
        /// The daemon stops its instances, then the agent is unregistered.
        case working
    }

    /// What the model is doing.
    public private(set) var phase = Phase.idle
    /// The plan the confirmation shows, or `nil` while no confirmation is open.
    public private(set) var plan: TurnOffPlan?
    /// Why the last Turn Off did not finish, or `nil`.
    public private(set) var message: String?

    @ObservationIgnored private let preview: () async throws(ClientError) -> StopAllPreview
    @ObservationIgnored private let atLoginCount: () -> Int
    @ObservationIgnored private let agentOff: (String) async -> TurnOffOutcome
    @ObservationIgnored private var daemonGone: () async -> Bool
    @ObservationIgnored private let unregister: () async -> Void
    @ObservationIgnored private let names: () -> [UUID: String]
    @ObservationIgnored private var retryKey: String?

    /// Creates a model.
    /// - Parameters:
    ///   - preview: Reads `instance.preview_stop_all`.
    ///   - atLoginCount: How many instances have the policy `at_login`, from the instance list.
    ///   - names: The names of the instances by ID, for the sentence of a refusal.
    ///   - agentOff: Sends `daemon.shutdown { agent_off }` with the idempotency key.
    ///   - daemonGone: Waits until the daemon went away; `false` when it did not in time.
    ///   - unregister: Unregisters the agent with the OS.
    public init(
        preview: @escaping () async throws(ClientError) -> StopAllPreview,
        atLoginCount: @escaping () -> Int,
        names: @escaping () -> [UUID: String],
        agentOff: @escaping (String) async -> TurnOffOutcome,
        daemonGone: @escaping () async -> Bool = { true },
        unregister: @escaping () async -> Void
    ) {
        self.preview = preview
        self.atLoginCount = atLoginCount
        self.names = names
        self.agentOff = agentOff
        self.daemonGone = daemonGone
        self.unregister = unregister
    }

    /// Replaces the wait for the daemon to go away. ``DaemonModel`` calls it once its
    /// presence exists.
    func attach(daemonGone: @escaping () async -> Bool) {
        self.daemonGone = daemonGone
    }

    /// Turn Off…: reads the preview and opens the confirmation.
    public func begin() async {
        guard phase == .idle else { return }
        phase = .loadingPreview
        message = nil
        do throws(ClientError) {
            let preview = try await preview()
            plan = TurnOffPlan(preview: preview, atLoginCount: atLoginCount())
        } catch {
            message = Self.body(of: error).message
        }
        phase = .idle
    }

    private static func body(of error: ClientError) -> ErrorBody {
        switch error {
        case .rejected(let body), .notSent(let body), .outcomeUnknown(let body): body
        }
    }

    /// The person closed the confirmation.
    public func cancel() {
        plan = nil
    }

    /// Turn Off: asks the daemon, and unregisters the agent when the daemon exits.
    public func confirm() async {
        guard phase == .idle, let plan, plan.canTurnOff else { return }
        phase = .working
        self.plan = nil
        message = nil
        let key = retryKey ?? UUID().uuidString.lowercased()
        retryKey = key
        switch await agentOff(key) {
        case .accepted(let accepted):
            retryKey = nil
            await finish(accepted)
        case .rejected(let body), .notSent(let body):
            retryKey = nil
            message = body.message
        case .outcomeUnknown(let body):
            // A retry reuses the key and gets the answer the daemon already has.
            message = body.message
        }
        phase = .idle
    }

    private func finish(_ accepted: ShutdownAccepted) async {
        guard accepted.exiting == true else {
            let result = accepted.stopAll ?? StopAllResult(stopped: [], failed: [], skipped: [])
            message = TurnOffPlan.refusal(result: result, names: names())
            return
        }
        guard await daemonGone() else {
            message = "The daemon is still stopping. Try Turn Off again in a moment."
            return
        }
        await unregister()
    }
}
