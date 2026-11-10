import Foundation
import StacklyClient
import Synchronization
import Testing

@testable import StacklyUI

/// The settings the Settings window keeps (spec 0032 AC-16, AC-17, AC-24): the last copy, the
/// revision every change carries, "Changed elsewhere", and the read that follows an event.
@Suite(.timeLimit(.minutes(1)))
struct SettingsStoreTests {
    /// A scripted daemon: the settings it holds, the calls it saw, and what it does next.
    nonisolated final class Daemon: Sendable {
        private struct State {
            var settings: DaemonSettingsView
            var calls: [Call] = []
            var keys: [String?] = []
            var failNext: ClientError?
            var gate: CheckedContinuation<Void, Never>?
            var holdUpdates = false
        }

        private let state: Mutex<State>

        init(_ settings: DaemonSettingsView = FixtureSettings.view()) {
            state = Mutex(State(settings: settings))
        }

        var calls: [Call] { state.withLock { $0.calls } }
        var keys: [String?] { state.withLock { $0.keys } }

        func hold() { state.withLock { $0.holdUpdates = true } }

        func release() {
            let waiting = state.withLock { state -> CheckedContinuation<Void, Never>? in
                state.holdUpdates = false
                defer { state.gate = nil }
                return state.gate
            }
            waiting?.resume()
        }

        func fail(next error: ClientError) { state.withLock { $0.failNext = error } }

        func replace(_ settings: DaemonSettingsView) { state.withLock { $0.settings = settings } }

        var client: SettingsClient {
            SettingsClient { [self] call, key, _ throws(ClientError) in
                state.withLock {
                    $0.calls.append(call)
                    $0.keys.append(key)
                }
                if case .settingsUpdate = call {
                    await waitIfHeld()
                }
                return try answer(call)
            }
        }

        private func waitIfHeld() async {
            await withCheckedContinuation { (continuation: CheckedContinuation<Void, Never>) in
                let held = state.withLock { state -> Bool in
                    guard state.holdUpdates else { return false }
                    state.gate = continuation
                    return true
                }
                if held == false { continuation.resume() }
            }
        }

        private func answer(_ call: Call) throws(ClientError) -> Reply {
            let failure = state.withLock { state -> ClientError? in
                defer { state.failNext = nil }
                return state.failNext
            }
            if let failure {
                switch call {
                case .settingsUpdate, .storageSetLocation: throw failure
                default: break
                }
            }
            switch call {
            case .settingsGet:
                return .settingsGet(state.withLock { $0.settings })
            case .storageListLocations:
                return .storageListLocations(LocationList(items: FixtureSettings.locations))
            case .settingsUpdate(let params):
                return state.withLock { state in
                    guard params.expectedRevision == state.settings.revision else {
                        return .settingsGet(state.settings)
                    }
                    state.settings = FixtureSettings.apply(params.patch, to: state.settings)
                    return .settingsUpdate(state.settings)
                }
            case .storageSetLocation(let params):
                return state.withLock { state in
                    let moved = FixtureSettings.location(
                        params.kind, number: 9,
                        path: {
                            if case .path(let path) = params.target {
                                path + "/Stackly"
                            } else {
                                nil
                            }
                        }(),
                        builtIn: params.target == .builtIn)
                    state.settings = FixtureSettings.moving(state.settings, to: moved)
                    return .storageSetLocation(moved)
                }
            default:
                throw .rejected(ClientErrorCatalog.protocolViolation())
            }
        }
    }

    static func mismatch() -> ClientError {
        .rejected(
            ErrorBody(
                code: "resource.revision_mismatch", category: .conflict,
                message: "settings changed", retryable: false, resource: nil, fields: nil,
                hint: nil, details: nil, operationId: nil, rule: nil, items: nil))
    }

    static func store(_ daemon: Daemon) -> SettingsStore {
        var count = 0
        return SettingsStore(
            client: daemon.client,
            newKey: {
                count += 1
                return "key-\(count)"
            })
    }

    // MARK: - Reading

    @Test func nothingIsKnownBeforeTheFirstRead() {
        let store = Self.store(Daemon())
        #expect(store.settings == nil)
        #expect(store.hasLoaded == false)
        #expect(store.defaultStartup == nil)
    }

    @Test func refreshReadsTheSettingsAndTheLocations() async {
        let store = Self.store(Daemon())
        await store.refresh()
        #expect(store.settings == FixtureSettings.view())
        #expect(store.locations == FixtureSettings.locations)
        #expect(store.hasLoaded)
    }

    @Test func aReadThatFailsKeepsTheLastKnownCopy() async {
        let daemon = Daemon()
        let store = Self.store(daemon)
        await store.refresh()
        let failing = SettingsStore(
            client: SettingsClient { _, _, _ throws(ClientError) in
                throw .notSent(ClientErrorCatalog.notConnected())
            })
        await failing.refresh()
        #expect(store.settings != nil)
        #expect(failing.settings == nil)
    }

    @Test func anOlderAnswerNeverReplacesANewerCopy() async {
        let daemon = Daemon(FixtureSettings.view(revision: 5))
        let store = Self.store(daemon)
        await store.refresh()
        daemon.replace(FixtureSettings.view(revision: 4))
        await store.refresh()
        #expect(store.settings?.revision == 5)
    }

    // MARK: - Changing

    @Test func aChangeCarriesTheRevisionThePatchAndAKeyThenShowsTheNewValue() async throws {
        let daemon = Daemon()
        let store = Self.store(daemon)
        await store.refresh()
        await store.apply(.startup(.atLogin))

        #expect(store.settings?.defaultStartup == .atLogin)
        #expect(store.settings?.revision == 4)
        #expect(store.saving == nil)
        #expect(store.notices.isEmpty)
        let update = try #require(
            daemon.calls.first {
                if case .settingsUpdate = $0 { return true } else { return false }
            })
        #expect(
            update
                == .settingsUpdate(
                    UpdateSettingsParams(
                        expectedRevision: 3,
                        patch: SettingsPatch(
                            defaultStartup: .atLogin, updateChannel: nil, telemetry: nil,
                            auditRetentionDays: nil))))
        #expect(daemon.keys.last == "key-1")
    }

    @Test func theRowShowsTheValueAskedForWhileTheChangeIsOnItsWay() async {
        let daemon = Daemon()
        let store = Self.store(daemon)
        await store.refresh()
        daemon.hold()
        let change = Task { await store.apply(.telemetry(true)) }
        while store.saving == nil { await Task.yield() }

        #expect(store.telemetry == true)
        #expect(store.settings?.telemetry == false)
        #expect(store.saving?.field == .telemetry)

        daemon.release()
        await change.value
        #expect(store.telemetry == true)
        #expect(store.saving == nil)
    }

    @Test func aSecondChangeWaitsOutTheFirst() async {
        let daemon = Daemon()
        let store = Self.store(daemon)
        await store.refresh()
        daemon.hold()
        let first = Task { await store.apply(.telemetry(true)) }
        while store.saving == nil { await Task.yield() }
        await store.apply(.channel(.beta))
        daemon.release()
        await first.value

        #expect(store.settings?.updateChannel == .stable)
        #expect(store.settings?.telemetry == true)
    }

    @Test func nothingIsSentBeforeTheFirstRead() async {
        let daemon = Daemon()
        let store = Self.store(daemon)
        await store.apply(.telemetry(true))
        #expect(daemon.calls.isEmpty)
    }

    @Test func aStaleRevisionShowsChangedElsewhereAndReadsTheNewValues() async {
        let daemon = Daemon()
        let store = Self.store(daemon)
        await store.refresh()
        daemon.replace(FixtureSettings.view(revision: 9, channel: .beta))
        daemon.fail(next: Self.mismatch())
        await store.apply(.startup(.atLogin))

        #expect(store.notices[.defaultStartup] == "Changed elsewhere. Try again.")
        #expect(store.settings?.revision == 9)
        #expect(store.updateChannel == .beta)
        #expect(store.defaultStartup == .manual)
    }

    @Test func aRefusalShowsTheDaemonsReasonAsASentence() async {
        let daemon = Daemon()
        let store = Self.store(daemon)
        await store.refresh()
        daemon.fail(
            next: .rejected(
                ErrorBody(
                    code: "request.invalid_params", category: .invalidInput,
                    message: "nothing to change", retryable: false, resource: nil, fields: nil,
                    hint: nil, details: nil, operationId: nil, rule: nil, items: nil)))
        await store.apply(.telemetry(true))

        #expect(store.notices[.telemetry] == "Nothing to change.")
        #expect(store.telemetry == false)
    }

    @Test func aChangeThatWasNotSentSaysSoAndKeepsTheOldValue() async {
        let daemon = Daemon()
        let store = Self.store(daemon)
        await store.refresh()
        daemon.fail(next: .notSent(ClientErrorCatalog.notConnected()))
        await store.apply(.telemetry(true))

        #expect(store.notices[.telemetry] == MutationFeedback.notSent)
        #expect(store.telemetry == false)
    }

    @Test func aChangeWithoutAnAnswerSaysItMayHaveRunAndReadsAgain() async {
        let daemon = Daemon()
        let store = Self.store(daemon)
        await store.refresh()
        daemon.replace(FixtureSettings.view(revision: 4, telemetry: true))
        daemon.fail(next: .outcomeUnknown(ClientErrorCatalog.notConnected()))
        await store.apply(.telemetry(true))

        #expect(store.notices[.telemetry] == MutationFeedback.outcomeUnknown)
        #expect(store.settings?.revision == 4)
        #expect(store.telemetry == true)
    }

    @Test func theNextChangeClearsTheLineOfTheLastOne() async {
        let daemon = Daemon()
        let store = Self.store(daemon)
        await store.refresh()
        daemon.fail(next: .notSent(ClientErrorCatalog.notConnected()))
        await store.apply(.telemetry(true))
        await store.apply(.telemetry(true))

        #expect(store.notices[.telemetry] == nil)
        #expect(store.settings?.telemetry == true)
    }

    // MARK: - Locations

    @Test func changingALocationSendsTheKindTheTargetAndTheRevisionThenReadsAgain() async throws {
        let daemon = Daemon()
        let store = Self.store(daemon)
        await store.refresh()
        await store.setLocation(.data, to: .path("/Volumes/Work"))

        let sent = try #require(
            daemon.calls.first {
                if case .storageSetLocation = $0 { return true } else { return false }
            })
        #expect(
            sent
                == .storageSetLocation(
                    SetLocationParams(
                        kind: .data, target: .path("/Volumes/Work"), expectedRevision: 3)))
        #expect(store.settings?.locations.data.path == "/Volumes/Work/Stackly")
        #expect(store.settings?.revision == 4)
        #expect(store.savingLocation == nil)
        #expect(store.locationNotices.isEmpty)
    }

    @Test func aFolderTheDaemonRefusesShowsTheRuleSentenceUnderTheRow() async {
        let daemon = Daemon()
        let store = Self.store(daemon)
        await store.refresh()
        daemon.fail(
            next: .rejected(
                ErrorBody(
                    code: "storage.location_invalid", category: .invalidInput,
                    message: "that folder is on an external volume", retryable: false,
                    resource: nil, fields: nil, hint: nil, details: nil, operationId: nil,
                    rule: nil, items: nil)))
        await store.setLocation(.cache, to: .path("/Volumes/USB"))

        #expect(store.locationNotices[.cache] == "That folder is on an external volume.")
        #expect(store.settings?.locations.cache.builtIn == true)
    }

    @Test func aStaleRevisionOnALocationShowsChangedElsewhere() async {
        let daemon = Daemon()
        let store = Self.store(daemon)
        await store.refresh()
        daemon.replace(FixtureSettings.view(revision: 8))
        daemon.fail(next: Self.mismatch())
        await store.setLocation(.logs, to: .builtIn)

        #expect(store.locationNotices[.logs] == "Changed elsewhere. Try again.")
        #expect(store.settings?.revision == 8)
    }

    @Test func nothingIsSentForALocationBeforeTheFirstRead() async {
        let daemon = Daemon()
        let store = Self.store(daemon)
        await store.setLocation(.data, to: .builtIn)
        #expect(daemon.calls.isEmpty)
    }

    // MARK: - Events

    @Test func aSettingsEventWithANewerRevisionReadsAgain() async throws {
        let daemon = Daemon()
        let store = Self.store(daemon)
        await store.refresh()
        daemon.replace(FixtureSettings.view(revision: 4, channel: .beta))

        let read = store.receive(event: FixtureSettings.event(revision: 4))
        await read?.value

        #expect(store.settings?.updateChannel == .beta)
    }

    @Test func aSettingsEventForARevisionWeHaveReadsNothing() async {
        let daemon = Daemon()
        let store = Self.store(daemon)
        await store.refresh()
        let before = daemon.calls.count

        let read = store.receive(event: FixtureSettings.event(revision: 3))
        #expect(read == nil)
        #expect(daemon.calls.count == before)
    }

    @Test func onlyASettingsEventReads() {
        let store = Self.store(Daemon())
        #expect(store.receive(event: FixtureSettings.event(revision: 1)) != nil)
        let other = Event(
            subscriptionId: 1, sequence: 2, at: FixtureClock.now, resource: nil, revision: nil,
            kind: .daemonReconciling(
                DaemonReconciling(
                    phase: .finished, instances: 0, adopted: 0, unknown: 0, restarting: 0,
                    findings: nil)))
        #expect(store.receive(event: other) == nil)
    }
}
