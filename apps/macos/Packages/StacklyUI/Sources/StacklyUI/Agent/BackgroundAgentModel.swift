import Foundation
import Observation

/// The state of the background agent and the four things the user can do about it (spec
/// 0015, *GUI*): Turn On, Open Login Items, Check Again, and Start stacklyd.
/// ``DaemonModel`` owns it and calls ``attach(presence:now:sleep:didAssess:didFail:systemRetry:)``
/// once. Reading the status never changes the registration: only ``turnOn()`` registers
/// and only ``start()`` relaunches. At most one call to the OS runs at a time.
@Observable
public final class BackgroundAgentModel {
    /// The state of the agent, or `nil` until the first status was read.
    public private(set) var assessment: AgentAssessment?
    /// When the status was last read, from the clock ``DaemonModel`` gave.
    public private(set) var lastCheckedAt: Date?
    /// What the last action failed with. Only an action sets it, and every action clears
    /// it first. Each time it is set to an error, ``attach(presence:now:sleep:didAssess:didFail:systemRetry:)``'s
    /// `didFail` is called once, after the `didAssess` of the same action's probe; clearing
    /// it calls nothing.
    public private(set) var lastError: AgentActionError?
    /// Whether an action (or the wait for the daemon after the agent turned on) runs. The
    /// banner buttons are disabled meanwhile.
    public private(set) var isWorking = false
    /// Whether the model waits for the daemon to answer after the agent is on.
    public private(set) var isStarting = false
    /// Whether the last probe was read right after a registration, so that `notFound` means
    /// the OS lost the service and `requiresApproval` means approval is still due.
    public private(set) var justRegistered = false
    /// The detail of an unavailable status for the banner: "timed out", "error 5", or
    /// "unknown status 7".
    public private(set) var unavailableDetail: String?
    /// The canonical path of the app bundle the OS answered for, or `nil`.
    public private(set) var bundlePath: String?
    /// When launchd last started the daemon, from the launch record.
    public private(set) var lastLaunchedAt: Date?

    @ObservationIgnored private let controller: any BackgroundAgentControlling
    @ObservationIgnored private let launchRecord: any LaunchRecordReading
    @ObservationIgnored private let rootOverrideActive: Bool
    @ObservationIgnored private let initialProbe: Result<AgentProbe, AgentActionError>?
    @ObservationIgnored private let initialLaunchRecord: LaunchRecordStatus

    @ObservationIgnored private var now: () -> Date = { Date() }
    @ObservationIgnored private var sleep: (Duration) async throws -> Void = {
        try await Task.sleep(for: $0)
    }
    @ObservationIgnored private var systemRetry: () -> Void = {}

    @ObservationIgnored private var presence: DaemonPresence = .connecting(slow: false)
    @ObservationIgnored private var appIsActive = true
    @ObservationIgnored private var isAttached = false
    @ObservationIgnored private var isStopped = false
    @ObservationIgnored private var actionRunning = false
    @ObservationIgnored private var refreshTask: Task<Void, Never>?
    @ObservationIgnored private var pollTask: Task<Void, Never>?
    @ObservationIgnored private var waitTask: Task<Void, Never>?
    @ObservationIgnored private var waiter: CheckedContinuation<Void, Never>?
    @ObservationIgnored private var sawHandshake = false

    /// Creates the model; ``DaemonModel`` attaches it.
    /// - Parameters:
    ///   - controller: The OS side.
    ///   - launchRecord: The reader of the launch record.
    ///   - rootOverrideActive: Whether `STACKLY_ROOT` is set. Then the model never calls
    ///     `controller` or `launchRecord`, and never polls.
    ///   - initialProbe: The answer of the first status, applied at once on attach with no
    ///     refresh at launch (fixtures); `nil` reads the status at launch.
    ///   - initialLaunchRecord: The launch record that goes with `initialProbe`.
    init(
        controller: any BackgroundAgentControlling,
        launchRecord: any LaunchRecordReading,
        rootOverrideActive: Bool,
        initialProbe: Result<AgentProbe, AgentActionError>? = nil,
        initialLaunchRecord: LaunchRecordStatus = .missing
    ) {
        self.controller = controller
        self.launchRecord = launchRecord
        self.rootOverrideActive = rootOverrideActive
        self.initialProbe = initialProbe
        self.initialLaunchRecord = initialLaunchRecord
    }

    // MARK: - Wiring

    /// Connects the model to the daemon model, once.
    /// Without root override, the initial probe is applied at once, or the status is read
    /// in the background. With root override, the assessment is `unavailable` with no call
    /// to the OS.
    func attach(
        presence: DaemonPresence,
        now: @escaping () -> Date,
        sleep: @escaping (Duration) async throws -> Void,

        systemRetry: @escaping () -> Void
    ) {
        guard isAttached == false else { return }
        isAttached = true
        self.presence = presence
        self.now = now
        self.sleep = sleep
        self.systemRetry = systemRetry
        if rootOverrideActive {
            assessment = BackgroundAgentClassifier.classify(
                AgentInput(
                    rootOverride: true, probe: .notChecked, launchRecord: false,
                    justRegistered: false))
            lastCheckedAt = now()
        } else if let initialProbe {
            apply(initialProbe, record: initialLaunchRecord, justRegistered: false)
        } else {
            startRefresh()
        }
        updatePolling()
    }

    /// The presence of the daemon changed. Starts a refresh when the daemon became
    /// unreachable, turns the polling on or off, and moves the wait for the daemon on.
    func presenceDidChange(_ presence: DaemonPresence) {
        let wasUnreachable = self.presence.isUnreachable
        self.presence = presence
        evaluateWait()
        if presence.isUnreachable, wasUnreachable == false, rootOverrideActive == false {
            startRefresh()
        }
        updatePolling()
    }

    /// The app went to the background: the polling stops, the wait for the daemon does not.
    func pausePolling() {
        appIsActive = false
        updatePolling()
    }

    /// The app became active: the polling starts again when its conditions hold.
    func resumePolling() {
        appIsActive = true
        updatePolling()
    }

    /// Cancels the polling, the wait for the daemon, and the refresh, and clears
    /// ``isWorking`` and ``isStarting``. Nothing is left running (spec 0013).
    func stop() {
        isStopped = true
        pollTask?.cancel()
        pollTask = nil
        refreshTask?.cancel()
        refreshTask = nil
        waitTask?.cancel()
        waitTask = nil
        isWorking = false
        isStarting = false
        waiter?.resume()
        waiter = nil
    }

    // MARK: - Refresh

    /// Reads the status again (Check Again). A refresh that runs now is joined, and an
    /// action in progress reports its own result, so this returns at once. Never registers.
    func refresh() async {
        guard rootOverrideActive == false, actionRunning == false else { return }
        await startRefresh()?.value
    }

    @discardableResult
    private func startRefresh() -> Task<Void, Never>? {
        guard rootOverrideActive == false, isStopped == false, actionRunning == false else {
            return nil
        }
        if let refreshTask { return refreshTask }
        let task = Task { [weak self] in
            guard let self else { return }
            let result = await self.readStatus()
            let record = await self.launchRecord.read()
            guard Task.isCancelled == false else { return }
            self.finishRefresh(result, record)
        }
        refreshTask = task
        return task
    }

    private func finishRefresh(
        _ result: Result<AgentProbe, AgentActionError>, _ record: LaunchRecordStatus
    ) {
        refreshTask = nil
        let before = assessment?.state
        apply(result, record: record, justRegistered: justRegistered, report: true)
        if actionRunning == false, isStarting == false, assessment?.state == .enabled,
            let before, before != .enabled, before != .unavailable
        {
            // launchd is starting the daemon: the permission was just given, or the
            // command line registered the agent.
            beginWait(waiter: nil)
        }
    }

    // MARK: - Actions

    /// Turn On: registers the agent. A result that needs approval opens Login Items once; an
    /// enabled one waits for the daemon. Returns when the whole action, the wait included,
    /// is over.
    func turnOn() async {
        await act(.register, opensSettings: true) {
            (agent) async throws(AgentActionError) -> AgentProbe in try await agent.register()
        }
    }

    /// Start stacklyd: registers again to start the job, then waits for the daemon like
    /// ``turnOn()``.
    func start() async {
        await act(.relaunch, opensSettings: false) {
            (agent) async throws(AgentActionError) -> AgentProbe in try await agent.relaunch()
        }
    }

    /// Open Login Items: opens System Settings. It changes nothing.
    func openLoginItems() {
        controller.openApprovalSettings()
    }

    private func act(
        _ op: AgentOp,
        opensSettings: Bool,
        _ call:
            @escaping (any BackgroundAgentControlling) async throws(AgentActionError) ->
            AgentProbe
    ) async {
        guard isWorking == false, rootOverrideActive == false, isStopped == false else { return }
        isWorking = true
        actionRunning = true
        lastError = nil
        if let task = refreshTask { await task.value }
        let outcome = await register(call)
        guard Task.isCancelled == false, isStopped == false else {
            actionRunning = false
            return
        }
        let record = await launchRecord.read()
        apply(outcome.result, record: record, justRegistered: outcome.justRegistered, report: true)
        if let error = outcome.error {
            fail(with: error)
        } else if case .success(.registration(.notRegistered, _)) = outcome.result {
            fail(with: .registrationNotKept)
        } else if case .success(.registration(.unrecognized, _)) = outcome.result {
            fail(with: .registrationNotKept)
        }
        actionRunning = false
        if opensSettings, case .success(.registration(.requiresApproval, _)) = outcome.result {
            controller.openApprovalSettings()
        }
        if assessment?.state == .enabled, lastError == nil {
            await withCheckedContinuation { continuation in
                beginWait(waiter: continuation)
            }
        } else {
            isWorking = false
        }
    }

    /// Keeps `error` as the Last error and reports it once.
    private func fail(with error: AgentActionError) {
        lastError = error
    }

    /// What a registration call left: the status to apply, whether it counts as just
    /// registered, and the error to show.
    private struct Registration {
        var result: Result<AgentProbe, AgentActionError>
        var justRegistered: Bool
        var error: AgentActionError?
    }

    private func register(
        _ call: (any BackgroundAgentControlling) async throws(AgentActionError) -> AgentProbe
    ) async -> Registration {
        do {
            return Registration(result: .success(try await call(controller)), justRegistered: true)
        } catch {
            // The OS refused. Most refusals leave a status that says what to do, so the
            // status is read again; only the ones without a state become the Last error.
            let reread = await readStatus()
            switch error {
            case .os(_, .alreadyRegistered, _, _), .os(_, .notFound, _, _):
                return Registration(result: reread, justRegistered: true)
            case .os(_, .launchDenied, _, _), .os(_, .mustBeEnabled, _, _):
                return Registration(result: reread, justRegistered: false)
            default:
                return Registration(result: reread, justRegistered: false, error: error)
            }
        }
    }

    // MARK: - Probe

    private func readStatus() async -> Result<AgentProbe, AgentActionError> {
        do {
            return .success(try await controller.status())
        } catch {
            return .failure(error)
        }
    }

    /// The one step every probe goes through: classify, then keep the facts the views show.
    private func apply(
        _ result: Result<AgentProbe, AgentActionError>,
        record: LaunchRecordStatus,
        justRegistered just: Bool,
        report: Bool = false
    ) {
        let outcome: ProbeOutcome =
            switch result {
            case .success(let probe): .probe(probe)
            case .failure(.os(_, _, _, let code)): .failed(code: code)
            case .failure: .failed(code: nil)
            }
        let assessment = BackgroundAgentClassifier.classify(
            AgentInput(
                rootOverride: rootOverrideActive, probe: outcome, launchRecord: record.exists,
                justRegistered: just))
        let checkedAt = now()
        self.assessment = assessment
        unavailableDetail = Self.detail(of: outcome)
        bundlePath = Self.bundle(of: outcome)
        lastLaunchedAt = record.lastLaunchedAt
        lastCheckedAt = checkedAt
        // `justRegistered` stays until a probe is neither `requiresApproval` nor `notFound`.
        if case .probe(.registration(let registration, _)) = outcome,
            registration == .requiresApproval || registration == .notFound
        {
            justRegistered = just
        } else {
            justRegistered = false
        }
        if isStarting { endWaitIfLaunchdStopped(assessment.state) }
    }

    /// A probe during the wait for the daemon that says the job will not start ends the
    /// wait with no error, so the user sees the real state at once. `unavailable` says
    /// nothing about the job, so the 30 second limit and the handshake rules still apply.
    private func endWaitIfLaunchdStopped(_ state: BackgroundAgentState) {
        switch state {
        case .requiresApproval, .disabled, .notRegistered, .notFound: endWait()
        case .enabled, .unavailable: break
        }
    }

    private static func detail(of outcome: ProbeOutcome) -> String? {
        switch outcome {
        case .failed(nil): "timed out"
        case .failed(let code?): "error \(code)"
        case .probe(.registration(.unrecognized(let value), _)): "unknown status \(value)"
        default: nil
        }
    }

    private static func bundle(of outcome: ProbeOutcome) -> String? {
        switch outcome {
        case .probe(.registration(_, let bundle)), .probe(.helperMissing(let bundle)): bundle
        default: nil
        }
    }

    // MARK: - The wait for the daemon

    private func beginWait(waiter: CheckedContinuation<Void, Never>?) {
        self.waiter = waiter
        isWorking = true
        isStarting = true
        sawHandshake = false
        systemRetry()
        waitTask?.cancel()
        let sleep = self.sleep
        waitTask = Task { [weak self] in
            do {
                try await sleep(AgentTiming.startWait)
            } catch {
                return
            }
            self?.startWaitElapsed()
        }
        evaluateWait()
    }

    private func startWaitElapsed() {
        guard isStarting, sawHandshake == false else { return }
        fail(with: .daemonDidNotStart(seconds: 30))
        endWait()
    }

    /// Moves the wait on after the presence changed: once the daemon answered, the wait
    /// ends when the presence is neither connecting, unreachable, nor refreshing, or when
    /// the daemon went away again.
    private func evaluateWait() {
        guard isStarting else { return }
        switch presence {
        case .refreshing:
            sawHandshake = true
        case .connecting, .unreachable:
            if sawHandshake, presence.isUnreachable { endWait() }
        default:
            sawHandshake = true
            endWait()
        }
    }

    private func endWait() {
        waitTask?.cancel()
        waitTask = nil
        isStarting = false
        isWorking = false
        let waiter = self.waiter
        self.waiter = nil
        waiter?.resume()
    }

    // MARK: - Polling

    private func updatePolling() {
        let shouldPoll =
            isAttached && isStopped == false && presence.isUnreachable && appIsActive
            && rootOverrideActive == false
        if shouldPoll {
            guard pollTask == nil else { return }
            let sleep = self.sleep
            pollTask = Task { [weak self] in
                while Task.isCancelled == false {
                    do {
                        try await sleep(AgentTiming.pollInterval)
                    } catch {
                        return
                    }
                    await self?.refresh()
                }
            }
        } else {
            pollTask?.cancel()
            pollTask = nil
        }
    }
}

extension DaemonPresence {
    /// Whether the presence is `unreachable`.
    fileprivate var isUnreachable: Bool {
        if case .unreachable = self { return true }
        return false
    }
}
