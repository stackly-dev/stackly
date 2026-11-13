import Foundation
import Observation
import StacklyClient
import Synchronization
import Testing

@testable import StacklyUI

/// The notifications of the app (spec 0032 AC-22): the four triggers of *Pemicu notifikasi*,
/// the three conditions that gate every post, the permission flow, and the click. Every test
/// runs over ``FakeNotificationPoster``, so none asks macOS for anything.
@Suite(.timeLimit(.minutes(1)))
struct NotificationModelTests {
    /// A model over a fake poster, with a defaults store of its own, a clock the test moves, and a
    /// wait the test releases.
    @MainActor final class Rig {
        let suite = "stackly.tests.notifications.\(UUID().uuidString)"
        let poster: FakeNotificationPoster
        let preferences: AppPreferences
        let model: NotificationModel
        /// What the closures of the model share with the test.
        final class Shared: Sendable {
            let active = Mutex(false)
            let clock = Mutex(Date(timeIntervalSince1970: 1_792_300_000))
            let opened = Mutex<[NotificationTarget]>([])
            let waits = Mutex<[CheckedContinuation<Void, any Error>]>([])
        }

        let shared = Shared()

        init(
            permission: NotificationPermission = .authorized, enabled: Bool = true,
            names: [UUID: String] = [:]
        ) {
            let defaults = UserDefaults(suiteName: suite) ?? .standard
            let poster = FakeNotificationPoster(permission: permission)
            let preferences = AppPreferences(defaults: defaults, apply: { _ in })
            preferences.notificationsEnabled = enabled
            self.poster = poster
            self.preferences = preferences
            let shared = shared
            model = NotificationModel(
                poster: poster, preferences: preferences,
                isAppActive: { shared.active.withLock { $0 } },
                now: { shared.clock.withLock { $0 } },
                sleep: { _ in
                    try await withCheckedThrowingContinuation { continuation in
                        shared.waits.withLock { $0.append(continuation) }
                    }
                })
            model.configure(
                names: { names[$0] }, titleOf: { _ in "Install PostgreSQL 17.6" },
                open: { target in shared.opened.withLock { $0.append(target) } })
        }

        /// Reads the permission, as the app does when it starts.
        func ready() async {
            await model.refreshPermission()
        }

        func advance(_ seconds: TimeInterval) {
            shared.clock.withLock { $0 = $0.addingTimeInterval(seconds) }
        }

        /// Lets the 15 second wait of an outage end.
        func finishWait() async {
            while shared.waits.withLock({ $0.isEmpty }) { await Task.yield() }
            let waiting = shared.waits.withLock { waits -> CheckedContinuation<Void, any Error> in
                waits.removeFirst()
            }
            waiting.resume()
            for _ in 0..<50 { await Task.yield() }
        }

        deinit { UserDefaults().removePersistentDomain(forName: suite) }
    }

    private static func orders() throws -> InstanceSummary { try ContractFixture.instance() }

    private static func failedCache() throws -> InstanceSummary {
        try ContractFixture.instance("frame-response-instance-get-failed")
    }

    private static func notReady(_ instance: InstanceSummary) -> InstanceSummary {
        instance.testing(
            readiness: InstanceReadiness(
                state: .notReady, reason: "probe_failed", detail: nil,
                observedAt: instance.readiness.observedAt, latencyMs: nil))
    }

    private static func operation(
        kind: OperationKind = .installRuntime, state: OperationState, seconds: TimeInterval
    ) throws -> OperationSummary {
        let base = try ContractFixture.operation(inEvent: "frame-event-operation-state-changed")
        let started = Date(timeIntervalSince1970: 1_792_300_000)
        return OperationSummary(
            id: base.id, kind: kind, resource: base.resource, state: state, stage: base.stage,
            progress: base.progress, waitingOn: nil, cancelRequestedAt: nil,
            pointOfNoReturnAt: nil, failure: nil, recovery: nil, result: nil,
            trigger: base.trigger, createdAt: started, startedAt: started,
            updatedAt: started.addingTimeInterval(seconds), revision: base.revision, cancel: nil,
            parent: nil)
    }

    // MARK: - A failed instance

    @Test func anInstanceThatFailsNotifiesOncePerTransition() async throws {
        let rig = await Rig()
        await rig.ready()
        let orders = try Self.orders()
        await rig.model.seed([orders])
        let failed = orders.testing(revision: 20, lifecycle: .failed)

        await rig.model.receive(.instanceState(failed))
        await rig.model.receive(.instanceState(failed))

        let posted = await rig.poster.posted
        #expect(posted.count == 1)
        #expect(posted.first?.title == "\(orders.name) failed")
        #expect(posted.first?.body == "Open Stackly to see what happened.")
        #expect(posted.first?.target == .instance(orders.id))
        #expect(
            posted.first?.identifier == "instance-failed-\(orders.id.uuidString.lowercased())-20")
    }

    @Test func anInstanceThatWasFailedInTheSnapshotDoesNotNotify() async throws {
        let rig = await Rig()
        await rig.ready()
        let cache = try Self.failedCache()
        await rig.model.seed([cache])
        await rig.model.receive(.instanceState(cache.testing(revision: cache.revision + 1)))
        #expect(await rig.poster.posted.isEmpty)
    }

    @Test func recoveryNeverNotifies() async throws {
        let rig = await Rig()
        await rig.ready()
        let cache = try Self.failedCache()
        await rig.model.seed([cache])
        await rig.model.receive(
            .instanceState(cache.testing(revision: cache.revision + 1, lifecycle: .running)))
        #expect(await rig.poster.posted.isEmpty)
    }

    // MARK: - Lost readiness

    @Test func aRunningInstanceThatStopsBeingReadyNotifiesOnce() async throws {
        let orders = try Self.orders()
        let rig = await Rig(names: [orders.id: orders.name])
        await rig.ready()
        await rig.model.seed([orders])
        let lost = Self.notReady(orders).healthChange(statusSeq: 9, health: .healthy)

        await rig.model.receive(
            .instanceHealth(lost, revision: orders.revision, sequence: 1))

        let posted = await rig.poster.posted
        #expect(posted.count == 1)
        #expect(posted.first?.title == "\(orders.name) isn’t ready")
        #expect(posted.first?.body == "It stopped accepting connections.")
        #expect(posted.first?.target == .instance(orders.id))
        #expect(
            posted.first?.identifier == "instance-unready-\(orders.id.uuidString.lowercased())-9")
    }

    @Test func lostReadinessNotifiesAtMostOncePerInstanceInFiveMinutes() async throws {
        let orders = try Self.orders()
        let rig = await Rig(names: [orders.id: orders.name])
        await rig.ready()
        await rig.model.seed([orders])
        let ready = orders.healthChange(statusSeq: 8, health: .healthy)
        let lost = Self.notReady(orders).healthChange(statusSeq: 9, health: .healthy)
        let ready2 = orders.healthChange(statusSeq: 10, health: .healthy)
        let lost2 = Self.notReady(orders).healthChange(statusSeq: 11, health: .healthy)
        let ready3 = orders.healthChange(statusSeq: 12, health: .healthy)
        let lost3 = Self.notReady(orders).healthChange(statusSeq: 13, health: .healthy)

        for change in [ready, lost, ready2, lost2] {
            await rig.model.receive(.instanceHealth(change, revision: 1, sequence: 1))
        }
        #expect(await rig.poster.posted.count == 1)

        rig.advance(5 * 60)
        for change in [ready3, lost3] {
            await rig.model.receive(.instanceHealth(change, revision: 1, sequence: 1))
        }
        #expect(await rig.poster.posted.count == 2)
    }

    @Test func lostReadinessOfAnInstanceThatIsNotRunningDoesNotNotify() async throws {
        let orders = try Self.orders()
        let rig = await Rig(names: [orders.id: orders.name])
        await rig.ready()
        await rig.model.seed([orders.testing(lifecycle: .stopping)])
        let lost = Self.notReady(orders).healthChange(statusSeq: 9, health: .healthy)
        await rig.model.receive(.instanceHealth(lost, revision: 1, sequence: 1))
        #expect(await rig.poster.posted.isEmpty)
    }

    @Test func anInstanceThatWasNotReadyInTheSnapshotDoesNotNotify() async throws {
        let orders = try Self.orders()
        let rig = await Rig(names: [orders.id: orders.name])
        await rig.ready()
        await rig.model.seed([Self.notReady(orders)])
        let lost = Self.notReady(orders).healthChange(statusSeq: 9, health: .healthy)
        await rig.model.receive(.instanceHealth(lost, revision: 1, sequence: 1))
        #expect(await rig.poster.posted.isEmpty)
    }

    // MARK: - A long operation

    @Test func aLongOperationThatEndedNotifiesWithItsTitleAndDuration() async throws {
        let rig = await Rig()
        await rig.ready()
        let done = try Self.operation(state: .succeeded, seconds: 42)

        await rig.model.receive(.operation(done))

        let posted = await rig.poster.posted
        #expect(posted.count == 1)
        #expect(posted.first?.title == "Install PostgreSQL 17.6 finished")
        #expect(posted.first?.body == "Took 42 seconds.")
        #expect(posted.first?.target == .operations)
        #expect(posted.first?.identifier == "operation-\(done.id.uuidString.lowercased())")
    }

    @Test func aFailedAndACancelledOperationSayTheirOutcome() async throws {
        let rig = await Rig()
        await rig.ready()
        await rig.model.receive(.operation(try Self.operation(state: .failed, seconds: 30)))
        await rig.model.receive(.operation(try Self.operation(state: .cancelled, seconds: 90)))

        let posted = await rig.poster.posted
        #expect(
            posted.map(\.title) == [
                "Install PostgreSQL 17.6 failed", "Install PostgreSQL 17.6 was cancelled",
            ])
        #expect(posted.last?.body == "It was cancelled after 1 minute.")
    }

    @Test func anOperationOfTenSecondsOrLessDoesNotNotify() async throws {
        let rig = await Rig()
        await rig.ready()
        await rig.model.receive(.operation(try Self.operation(state: .succeeded, seconds: 10)))
        #expect(await rig.poster.posted.isEmpty)
    }

    @Test func anOperationThatHasNotEndedDoesNotNotify() async throws {
        let rig = await Rig()
        await rig.ready()
        await rig.model.receive(.operation(try Self.operation(state: .running, seconds: 60)))
        #expect(await rig.poster.posted.isEmpty)
    }

    @Test func aKindOutsideThePopoverListDoesNotNotify() async throws {
        let rig = await Rig()
        await rig.ready()
        for kind in [OperationKind.updateDaemonSettings, .setStorageLocation, .setInstanceStartup] {
            await rig.model.receive(
                .operation(try Self.operation(kind: kind, state: .succeeded, seconds: 60)))
        }
        #expect(await rig.poster.posted.isEmpty)
    }

    // MARK: - The daemon is out of reach

    @Test func theDaemonNotifiesOnceAfterFifteenSecondsOutOfReach() async {
        let rig = await Rig()
        await rig.ready()
        await rig.model.connectionChanged(isReachable: true)
        await rig.model.connectionChanged(isReachable: false)
        await rig.model.connectionChanged(isReachable: false)
        await rig.finishWait()

        let posted = await rig.poster.posted
        #expect(posted.count == 1)
        #expect(posted.first?.title == "stacklyd isn’t reachable")
        #expect(posted.first?.body == "Your instances keep running if stacklyd comes back.")
        #expect(posted.first?.target == .diagnostics)
        #expect(posted.first?.identifier.hasPrefix("daemon-unreachable-") == true)
    }

    @Test func aDaemonThatComesBackInTimeDoesNotNotify() async {
        let rig = await Rig()
        await rig.ready()
        await rig.model.connectionChanged(isReachable: true)
        await rig.model.connectionChanged(isReachable: false)
        await rig.model.connectionChanged(isReachable: true)
        await Task.yield()
        #expect(await rig.poster.posted.isEmpty)
    }

    @Test func aDaemonThatWasNeverReachableDoesNotNotify() async {
        let rig = await Rig()
        await rig.ready()
        await rig.model.connectionChanged(isReachable: false)
        #expect(rig.shared.waits.withLock { $0.isEmpty })
        #expect(await rig.poster.posted.isEmpty)
    }

    @Test func aSecondOutageNotifiesAgain() async {
        let rig = await Rig()
        await rig.ready()
        await rig.model.connectionChanged(isReachable: true)
        await rig.model.connectionChanged(isReachable: false)
        await rig.finishWait()
        rig.advance(120)
        await rig.model.connectionChanged(isReachable: true)
        await rig.model.connectionChanged(isReachable: false)
        await rig.finishWait()
        #expect(await rig.poster.posted.count == 2)
    }

    // MARK: - The gates

    @Test func nothingIsPostedWhileTheToggleIsOff() async throws {
        let rig = await Rig(enabled: false)
        await rig.ready()
        await rig.model.seed([try Self.orders()])
        await rig.model.receive(
            .instanceState(try Self.orders().testing(revision: 20, lifecycle: .failed)))
        #expect(await rig.poster.posted.isEmpty)
    }

    @Test func nothingIsPostedWithoutPermission() async throws {
        for permission in [NotificationPermission.denied, .notDetermined] {
            let rig = await Rig(permission: permission)
            await rig.ready()
            await rig.model.seed([try Self.orders()])
            await rig.model.receive(
                .instanceState(try Self.orders().testing(revision: 20, lifecycle: .failed)))
            #expect(await rig.poster.posted.isEmpty)
            #expect(
                await rig.poster.calls.contains { if case .post = $0 { true } else { false } }
                    == false)
        }
    }

    @Test func nothingIsPostedBeforeThePermissionWasRead() async throws {
        let rig = await Rig()
        await rig.model.seed([try Self.orders()])
        await rig.model.receive(
            .instanceState(try Self.orders().testing(revision: 20, lifecycle: .failed)))
        #expect(await rig.poster.posted.isEmpty)
    }

    @Test func nothingIsPostedWhileTheAppIsActive() async throws {
        let rig = await Rig()
        await rig.ready()
        rig.shared.active.withLock { $0 = true }
        await rig.model.seed([try Self.orders()])
        await rig.model.receive(
            .instanceState(try Self.orders().testing(revision: 20, lifecycle: .failed)))
        #expect(await rig.poster.posted.isEmpty)
    }

    @Test func anotherEventNotifiesNothing() async {
        let rig = await Rig()
        await rig.ready()
        await rig.model.receive(FixtureSettings.event(revision: 4))
        #expect(await rig.poster.posted.isEmpty)
    }

    // MARK: - Permission

    @Test func turningOnAsksMacOSOnceAndKeepsTheAnswer() async {
        let rig = await Rig(permission: .notDetermined, enabled: false)
        await rig.model.setEnabled(true)
        #expect(rig.preferences.notificationsEnabled)
        #expect(rig.model.permission == .authorized)
        await rig.model.setEnabled(false)
        await rig.model.setEnabled(true)
        let asks = await rig.poster.calls.filter { $0 == .requestPermission }.count
        #expect(asks == 1)
    }

    @Test func aRefusalKeepsTheToggleOnAndNothingIsPosted() async throws {
        let rig = await Rig(permission: .notDetermined, enabled: false)
        rig.poster.setPermission(.notDetermined)
        let refusing = FakeNotificationPoster(permission: .notDetermined, answer: .denied)
        let defaults = UserDefaults(suiteName: rig.suite) ?? .standard
        let preferences = AppPreferences(defaults: defaults, apply: { _ in })
        let model = await NotificationModel(
            poster: refusing, preferences: preferences, isAppActive: { false })

        await model.setEnabled(true)

        #expect(await preferences.notificationsEnabled)
        #expect(await model.permission == .denied)
        await model.seed([try Self.orders()])
        await model.receive(
            .instanceState(try Self.orders().testing(revision: 20, lifecycle: .failed)))
        #expect(refusing.posted.isEmpty)
    }

    @Test func aPermissionAlreadyDeniedIsNotAskedAgain() async {
        let rig = await Rig(permission: .denied, enabled: false)
        await rig.model.setEnabled(true)
        #expect(rig.model.permission == .denied)
        #expect(await rig.poster.calls.contains(.requestPermission) == false)
    }

    @Test func theToggleSurvivesALaunch() async {
        let rig = await Rig(enabled: true)
        let again = await AppPreferences(
            defaults: UserDefaults(suiteName: rig.suite) ?? .standard, apply: { _ in })
        #expect(await again.notificationsEnabled)
    }

    @Test func aChangeInSystemSettingsShowsAfterTheNextRead() async {
        let rig = await Rig(permission: .authorized)
        await rig.ready()
        #expect(rig.model.permission == .authorized)
        rig.poster.setPermission(.denied)
        await rig.model.refreshPermission()
        #expect(rig.model.permission == .denied)
    }

    @Test func openingSystemSettingsGoesThroughThePort() async {
        let rig = await Rig()
        rig.model.openSystemSettings()
        #expect(await rig.poster.calls.contains(.openSystemSettings))
    }

    // MARK: - Clicks

    @Test func beginInstallsTheClickHandlerAndReadsThePermission() async {
        let rig = await Rig()
        rig.model.begin()
        while rig.model.permission == nil { await Task.yield() }
        let calls = await rig.poster.calls
        #expect(calls.contains(.installClickHandler))
        #expect(calls.contains(.permission))
    }

    @Test func aClickOpensTheMainWindowOnThePageItNames() async {
        let rig = await Rig()
        rig.model.begin()
        let id = UUID()

        rig.poster.click(.instance(id))

        #expect(rig.shared.opened.withLock { $0 } == [.instance(id)])
        #expect(rig.model.pending?.target == .instance(id))
        #expect(rig.model.takePending() == .instance(id))
        #expect(rig.model.pending == nil)
        #expect(rig.model.takePending() == nil)
    }

    @Test func everyTargetNamesItsPage() {
        let id = UUID()
        #expect(NotificationTarget.instance(id).destination == .instance(id))
        #expect(NotificationTarget.operations.destination == .operations)
        #expect(NotificationTarget.diagnostics.destination == .diagnostics)
    }

    @Test func aTargetSurvivesTheTripThroughMacOS() {
        let id = UUID()
        for target in [NotificationTarget.instance(id), .operations, .diagnostics] {
            #expect(NotificationTarget(userInfo: target.userInfo) == target)
        }
        #expect(NotificationTarget(userInfo: ["target": "instance"]) == nil)
        #expect(NotificationTarget(userInfo: ["target": "elsewhere"]) == nil)
        #expect(NotificationTarget(userInfo: [:]) == nil)
    }

    // MARK: - The words

    @Test func durationsReadInSecondsAndMinutes() {
        #expect(NotificationContent.durationText(1) == "1 second")
        #expect(NotificationContent.durationText(42) == "42 seconds")
        #expect(NotificationContent.durationText(60) == "1 minute")
        #expect(NotificationContent.durationText(185) == "3 minutes")
    }

    @Test func theOperationListIsTheOneOfThePopover() {
        #expect(
            NotificationContent.operationKinds == [
                .installRuntime, .createInstance, .startInstance, .stopInstance,
                .restartInstance, .stopAllInstances,
            ])
    }

    @Test func theInertModelNeverTouchesMacOS() async throws {
        let rig = await Rig()
        let inert = await NotificationModel.inert(preferences: rig.preferences)
        await inert.setEnabled(true)
        await inert.refreshPermission()
        #expect(await inert.permission == .denied)
    }

    // MARK: - Following the shell

    @Observable final class Reachability {
        var isReachable = false
    }

    /// Lets the tasks on the main actor run until `done` holds.
    private static func settle(_ done: () -> Bool) async {
        var turns = 0
        while done() == false, turns < 4000 {
            await Task.yield()
            turns += 1
        }
    }

    @Test func attachedToTheShellASnapshotSeedsAndAnEventNotifies() async throws {
        let orders = try Self.orders()
        let rig = Rig()
        await rig.ready()
        let sync = ShellSync()
        let instances = InstanceStore(
            fetchPage: { _ throws(ClientError) in Page(items: [orders], next: nil) },
            fetchOne: { _ throws(ClientError) in nil })
        sync.register(instances.store)
        let reachability = Reachability()
        rig.model.attach(
            sync: sync, instances: instances, titleOf: { _ in "Install" },
            isReachable: { reachability.isReachable }, open: { _ in })

        sync.handle(.resnapshot(.gap(.cursorExpired)))
        await Self.settle { rig.model.knownInstanceCount == 1 }
        #expect(rig.model.knownInstanceCount == 1)
        #expect(rig.poster.posted.isEmpty, "the snapshot itself never notifies")

        sync.handle(.event(.instanceState(orders.testing(revision: 30, lifecycle: .failed))))
        await Self.settle { rig.poster.posted.count == 1 }
        #expect(rig.poster.posted.first?.title == "\(orders.name) failed")
        rig.model.stop()
    }

    @Test func attachedToTheShellAnOutageNotifiesAfterTheWait() async {
        let rig = Rig()
        await rig.ready()
        let sync = ShellSync()
        let instances = InstanceStore(
            fetchPage: { _ throws(ClientError) in Page(items: [], next: nil) },
            fetchOne: { _ throws(ClientError) in nil })
        let reachability = Reachability()
        reachability.isReachable = true
        rig.model.attach(
            sync: sync, instances: instances, titleOf: { _ in "Install" },
            isReachable: { reachability.isReachable }, open: { _ in })
        await Task.yield()

        reachability.isReachable = false
        await rig.finishWait()
        await Self.settle { rig.poster.posted.count == 1 }

        #expect(rig.poster.posted.first?.title == "stacklyd isn’t reachable")
        rig.model.stop()
    }
}
