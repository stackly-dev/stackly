import AppKit
import Foundation
import Observation
import StacklyClient

/// The notifications of the app (spec 0032 AC-22): permission, the four triggers, and what a
/// click opens.
///
/// It posts only when the person turned notifications on (``AppPreferences``), macOS gave
/// permission, and Stackly is not the active app. State that is already in the snapshot never
/// notifies: the model follows the lifecycle and readiness of each instance itself, fills
/// them from the stores after every snapshot, and compares each event with what it knew
/// before. There is no notification for a recovery. Nothing here is sent to the daemon.
@Observable
public final class NotificationModel {
    /// A request to open a page, left by a click until the main window shows it.
    public struct Pending: Equatable {
        /// Tells one request from another.
        public let id = UUID()
        /// What to open.
        let target: NotificationTarget
    }

    /// What macOS says, or `nil` before the first read.
    public private(set) var permission: NotificationPermission?
    /// The page a click asked for, until the main window takes it.
    public private(set) var pending: Pending?

    @ObservationIgnored private let poster: any NotificationPosting
    @ObservationIgnored private let preferences: AppPreferences
    @ObservationIgnored private let isAppActive: () -> Bool
    @ObservationIgnored private let now: () -> Date
    @ObservationIgnored private let sleep: (Duration) async throws -> Void
    @ObservationIgnored private var lifecycles: [UUID: InstanceLifecycle] = [:]
    @ObservationIgnored private var ready: [UUID: Bool] = [:]
    @ObservationIgnored private var lastUnready: [UUID: Date] = [:]
    @ObservationIgnored private var wasConnected = false
    @ObservationIgnored private var outage: (since: Date, task: Task<Void, Never>)?
    @ObservationIgnored private var tasks: [Task<Void, Never>] = []
    @ObservationIgnored private var names: (UUID) -> String? = { _ in nil }
    @ObservationIgnored private var titleOf: (OperationSummary) -> String = { _ in "Operation" }
    @ObservationIgnored private var open: @MainActor (NotificationTarget) -> Void = { _ in }

    /// Creates a model that posts nothing until ``attach`` and ``begin()``.
    /// - Parameters:
    ///   - poster: The macOS side.
    ///   - preferences: Where the toggle lives.
    ///   - isAppActive: Whether Stackly is the active app; `NSApplication.isActive` by default.
    ///   - now: The wall clock.
    ///   - sleep: Waits; tests inject one they release.
    init(
        poster: any NotificationPosting, preferences: AppPreferences,
        isAppActive: @escaping () -> Bool = { NSApplication.shared.isActive },
        now: @escaping () -> Date = { Date() },
        sleep: @escaping (Duration) async throws -> Void = { try await Task.sleep(for: $0) }
    ) {
        self.poster = poster
        self.preferences = preferences
        self.isAppActive = isAppActive
        self.now = now
        self.sleep = sleep
    }

    /// The live model over `UNUserNotificationCenter`.
    /// - Parameter preferences: Where the toggle lives.
    public convenience init(preferences: AppPreferences) {
        self.init(poster: UserNotificationPoster(), preferences: preferences)
    }

    /// A model that never touches macOS and never posts, for tests that build a shell and for
    /// previews. It has no poster, so it also never asks for permission.
    public static func inert(preferences: AppPreferences) -> NotificationModel {
        NotificationModel(poster: InertNotificationPoster(), preferences: preferences)
    }

    // MARK: - Wiring

    /// Connects the model to the shell: the stores it reads, the names it writes, the page
    /// a click opens, and the events and snapshots it follows.
    /// - Parameters:
    ///   - sync: The shell's sync, for events and snapshot rounds.
    ///   - instances: The instances the model fills its picture from.
    ///   - titleOf: The title of an operation.
    ///   - isReachable: Whether the daemon answers now.
    ///   - open: Opens a page of the main window.
    @MainActor func attach(
        sync: ShellSync, instances: InstanceStore, titleOf: @escaping (OperationSummary) -> String,
        isReachable: @escaping () -> Bool, open: @escaping @MainActor (NotificationTarget) -> Void
    ) {
        configure(
            names: { id in instances.store.items[id]?.name }, titleOf: titleOf, open: open)
        let events = sync.events(kindPrefix: "")
        tasks.append(
            Task { [weak self] in
                for await event in events {
                    await self?.receive(event)
                }
            })
        tasks.append(
            Task { [weak self] in
                let rounds = Observations { sync.completedRounds }
                for await _ in rounds {
                    self?.seed(instances.instances)
                }
            })
        tasks.append(
            Task { [weak self] in
                let reachable = Observations { isReachable() }
                for await value in reachable {
                    self?.connectionChanged(isReachable: value)
                }
            })
    }

    /// Sets what the model asks the shell for: the name of an instance, the title of an
    /// operation, and the page a click opens.
    @MainActor func configure(
        names: @escaping (UUID) -> String?, titleOf: @escaping (OperationSummary) -> String,
        open: @escaping @MainActor (NotificationTarget) -> Void
    ) {
        self.names = names
        self.titleOf = titleOf
        self.open = open
    }

    /// Installs the click handler and reads the permission. The app delegate calls it when the
    /// app finished launching, before the first event is handled, so the click that launched
    /// the app is handled too.
    @MainActor public func begin() {
        poster.installClickHandler { [weak self] target in self?.click(target) }
        Task { await refreshPermission() }
    }

    /// Stops following the shell.
    func stop() {
        for task in tasks { task.cancel() }
        tasks.removeAll()
        outage?.task.cancel()
        outage = nil
    }

    // MARK: - Permission

    /// Reads the permission again. The app does it every time it becomes active, so a change
    /// in System Settings shows at once.
    public func refreshPermission() async {
        permission = await poster.permission()
    }

    /// Turns notifications on or off. Turning them on asks macOS for permission the first time;
    /// a refusal keeps the toggle on, and the row says why nothing comes.
    public func setEnabled(_ enabled: Bool) async {
        preferences.notificationsEnabled = enabled
        guard enabled else { return }
        if await poster.permission() == .notDetermined {
            permission = await poster.requestPermission()
        } else {
            await refreshPermission()
        }
    }

    /// Opens the notification settings of Stackly in System Settings.
    public func openSystemSettings() {
        poster.openSystemSettings()
    }

    // MARK: - Clicks

    /// A click on a notification: the page it names opens in the main window.
    @MainActor func click(_ target: NotificationTarget) {
        pending = Pending(target: target)
        open(target)
    }

    /// Takes the page a click asked for, once the main window shows it.
    func takePending() -> ShellDestination? {
        defer { pending = nil }
        return pending?.target.destination
    }

    // MARK: - Following

    /// How many instances the model follows, for tests.
    var knownInstanceCount: Int { lifecycles.count }

    /// Fills the picture of the instances from `instances`, after a snapshot. Nothing in it
    /// notifies.
    func seed(_ instances: [InstanceSummary]) {
        lifecycles = Dictionary(
            instances.map { ($0.id, $0.lifecycle) }, uniquingKeysWith: { first, _ in first })
        ready = Dictionary(
            instances.map { ($0.id, $0.readiness.state == .ready) },
            uniquingKeysWith: { first, _ in first })
    }

    /// Takes one event: a failed instance, lost readiness, or a long operation that ended.
    func receive(_ event: Event) async {
        switch event.kind {
        case .instanceStateChanged(let changed):
            let instance = changed.instance
            let before = lifecycles[instance.id]
            lifecycles[instance.id] = instance.lifecycle
            ready[instance.id] = instance.readiness.state == .ready
            if instance.lifecycle == .failed, before != .failed {
                await post(NotificationContent.failed(instance))
            }
        case .instanceHealthChanged(let health):
            let wasReady = ready[health.instanceId] ?? false
            let isReady = health.readiness.state == .ready
            ready[health.instanceId] = isReady
            guard wasReady, health.readiness.state == .notReady,
                lifecycles[health.instanceId] == .running,
                let name = names(health.instanceId), unreadyIsDue(health.instanceId)
            else { return }
            lastUnready[health.instanceId] = now()
            await post(
                NotificationContent.unready(
                    name: name, id: health.instanceId, statusSeq: health.statusSeq))
        case .operationStateChanged(let operation):
            guard NotificationContent.operationKinds.contains(operation.kind),
                let started = operation.startedAt,
                operation.updatedAt.timeIntervalSince(started) > NotificationContent.longOperation,
                let notification = NotificationContent.operation(
                    operation, title: titleOf(operation),
                    duration: operation.updatedAt.timeIntervalSince(started))
            else { return }
            await post(notification)
        default:
            break
        }
    }

    /// Follows the connection. The daemon notifies once it stayed out of reach for 15
    /// seconds after it was reachable in this run.
    func connectionChanged(isReachable: Bool) {
        if isReachable {
            wasConnected = true
            outage?.task.cancel()
            outage = nil
            return
        }
        guard wasConnected, outage == nil else { return }
        let since = now()
        let task = Task { [weak self, sleep] in
            do {
                try await sleep(NotificationContent.unreachableDelay)
            } catch {
                return
            }
            await self?.outageLasted(since: since)
        }
        outage = (since, task)
    }

    private func outageLasted(since: Date) async {
        guard let outage, outage.since == since else { return }
        await post(NotificationContent.unreachable(since: since))
    }

    private func unreadyIsDue(_ id: UUID) -> Bool {
        guard let last = lastUnready[id] else { return true }
        return now().timeIntervalSince(last) >= NotificationContent.unreadyInterval
    }

    /// Posts `notification` when the toggle is on, macOS gave permission, and Stackly is not the
    /// active app.
    private func post(_ notification: AppNotification) async {
        guard preferences.notificationsEnabled, permission == .authorized,
            isAppActive() == false
        else { return }
        await poster.post(notification)
    }
}

/// A poster that does nothing, behind ``NotificationModel/inert(preferences:)``.
nonisolated private struct InertNotificationPoster: NotificationPosting {
    func permission() async -> NotificationPermission { .denied }
    func requestPermission() async -> NotificationPermission { .denied }
    func post(_ notification: AppNotification) async {}
    func openSystemSettings() {}
    func installClickHandler(_ handler: @escaping @MainActor @Sendable (NotificationTarget) -> Void)
    {}
}
