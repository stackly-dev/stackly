import Foundation
import StacklyClient
import StacklyTestSupport
import Testing

@testable import StacklyUI

/// The environment store: `env.list` in one page, and the reload rules of spec 0044 *Event*.
@MainActor
struct EnvironmentStoreTests {
    /// Counts the reads the store sends.
    final class Script {
        var listCalls = 0
        var getCalls: [EnvGetParams] = []
        var items = FixtureEnvironments.summaries

        func call(_ call: Call) throws(ClientError) -> Reply {
            switch call {
            case .envList:
                listCalls += 1
                return .envList(EnvironmentList(items: items))
            case .envGet(let params):
                getCalls.append(params)
                guard let detail = FixtureEnvironments.detail(params.id) else {
                    throw .rejected(PageRig.unknownMethod)
                }
                return .envGet(detail)
            default:
                throw .rejected(PageRig.unknownMethod)
            }
        }
    }

    let script = Script()

    private func store(
        sleep: @escaping (Duration) async throws -> Void = { _ in }
    ) -> EnvironmentStore {
        let script = script
        return EnvironmentStore(
            call: { call throws(ClientError) in try script.call(call) },
            now: { FixtureClock.now }, sleep: sleep)
    }

    private func event(_ kind: EventKind, sequence: UInt64 = 1) -> Event {
        Event(
            subscriptionId: 1, sequence: sequence, at: FixtureClock.now, resource: nil,
            revision: nil, kind: kind)
    }

    private func operation(_ kind: ResourceKind, state: OperationState) -> EventKind {
        .operationStateChanged(
            .fixture(
                kind: .importEnvironment,
                resource: ResourceRef(
                    kind: kind, id: FixtureEnvironments.erpID.uuidString.lowercased()),
                state: state))
    }

    @Test func aSnapshotListsTheEnvironmentsByName() async {
        let store = store()
        #expect(store.environments == nil)
        await store.store.resnapshot()
        #expect(store.environments?.map(\.name) == ["billing", "erp-development", "reporting"])
        #expect(store.subtitle == "3 projects")
        #expect(store.environment(FixtureEnvironments.erpID)?.readyCount == 1)
    }

    @Test(
        .evidence(
            covers: ["REQ-020"],
            expected:
                "an environment operation that ended loads env.list again at once; an operation of another resource does not"
        ))
    func anEndedEnvironmentOperationLoadsTheListAgain() async throws {
        let store = store(sleep: { _ in try await Task.sleep(for: .seconds(3_600)) })
        await store.store.resnapshot()
        store.store.receive(event(operation(.instance, state: .succeeded), sequence: 2))
        for _ in 0..<10 { await Task.yield() }
        #expect(script.listCalls == 1)
        store.store.receive(event(operation(.environment, state: .succeeded), sequence: 3))
        try await waitUntil { script.listCalls == 2 }
        #expect(store.changes >= 2)
        #expect(store.lastEnded?.environment == FixtureEnvironments.erpID)
    }

    @Test(
        .evidence(
            covers: ["REQ-021", "NFR-012"],
            expected:
                "a running environment operation, its progress, and a state change of its child load env.list again after 300 ms, coalesced into one load"
        ))
    func aRunningUpItsProgressAndItsChildrenLoadTheListAfterTheDelay() async throws {
        var waits: [Duration] = []
        script.items = FixtureEnvironmentUp.bringingUpSummaries
        let store = store(sleep: { waits.append($0) })
        await store.store.resnapshot()
        #expect(store.activeOperations == [FixtureEnvironmentUp.upID])

        store.store.receive(event(.operationStateChanged(FixtureEnvironmentUp.upOperation())))
        try await waitUntil { script.listCalls == 2 }
        store.store.receive(
            event(.operationProgress(FixtureEnvironmentUp.upOperation(done: 2)), sequence: 2))
        try await waitUntil { script.listCalls == 3 }
        let child = FixtureEnvironmentUp.child(
            1, kind: .startInstance, instance: FixtureEnvironments.instances[0],
            state: .running)
        store.store.receive(event(.operationStateChanged(child), sequence: 3))
        try await waitUntil { script.listCalls == 4 }
        #expect(waits.allSatisfy { $0 == .milliseconds(300) })
        #expect(waits.count == 3)
    }

    @Test func aBurstOfProgressLoadsTheListOnce() async throws {
        let store = store(sleep: { _ in try await Task.sleep(for: .milliseconds(40)) })
        await store.store.resnapshot()
        for sequence in 1...5 {
            store.store.receive(
                event(
                    .operationProgress(
                        FixtureEnvironmentUp.upOperation(done: UInt64(sequence % 4))),
                    sequence: UInt64(sequence)))
        }
        try await waitUntil { script.listCalls == 2 }
        try await Task.sleep(for: .milliseconds(120))
        #expect(script.listCalls == 2)
    }

    @Test func aChildOfAnotherOperationOrTheProgressOfAnInstanceLoadsNothing() async throws {
        let store = store()
        await store.store.resnapshot()
        let child = FixtureEnvironmentUp.child(
            1, kind: .startInstance, instance: FixtureEnvironments.instances[0],
            state: .running)
        store.store.receive(event(.operationStateChanged(child)))
        store.store.receive(
            event(
                .operationProgress(
                    .fixture(
                        kind: .installRuntime,
                        resource: ResourceRef(kind: .runtime, id: "r"), state: .running)),
                sequence: 2))
        for _ in 0..<20 { await Task.yield() }
        #expect(script.listCalls == 1)
    }

    @Test(
        .evidence(
            covers: ["REQ-021", "NFR-012"],
            expected:
                "an Up another client started shows Bringing up… after its first operation event, without an instance event"
        ))
    func anUpStartedByAnotherClientShowsBringingUpWithoutAnInstanceEvent() async throws {
        let store = store()
        await store.store.resnapshot()
        let erp = try #require(store.environment(FixtureEnvironments.erpID))
        #expect(StatusAppearance.environment(erp).label == "Partially Ready")

        script.items = FixtureEnvironmentUp.bringingUpSummaries
        store.store.receive(
            event(.operationStateChanged(FixtureEnvironmentUp.upOperation(done: 0))))
        try await waitUntil { store.environment(FixtureEnvironments.erpID)?.activeOperation != nil }
        let running = try #require(store.environment(FixtureEnvironments.erpID))
        #expect(StatusAppearance.environment(running).label == "Bringing up…")
    }

    @Test func instanceEventsLoadTheListOnceAfterTheyStop() async throws {
        let store = store(sleep: { _ in try await Task.sleep(for: .milliseconds(40)) })
        await store.store.resnapshot()
        let removed = EventKind.instanceRemoved(
            InstanceRemoved(instanceId: FixtureEnvironments.instances[0].id))
        for sequence in 1...3 {
            store.store.receive(event(removed, sequence: UInt64(sequence)))
        }
        try await waitUntil { script.listCalls == 2 }
        try await Task.sleep(for: .milliseconds(120))
        #expect(script.listCalls == 2)
    }

    @Test func theStoreWaitsTheSpecDelayForInstanceEvents() async throws {
        var waits: [Duration] = []
        let store = store(sleep: { waits.append($0) })
        await store.store.resnapshot()
        store.store.receive(
            event(
                .instanceRemoved(
                    InstanceRemoved(
                        instanceId: FixtureEnvironments.instances[0].id))))
        try await waitUntil { script.listCalls == 2 }
        #expect(waits.contains(.milliseconds(300)))
    }

    @Test func theDetailReadsTheManifestOnlyWhenAsked() async throws {
        let store = store()
        _ = try await store.detail(FixtureEnvironments.erpID, checkManifest: false)
        _ = try await store.detail(FixtureEnvironments.erpID, checkManifest: true)
        #expect(script.getCalls.map(\.checkManifest) == [false, true])
    }
}

/// The sidebar row a window restores, and the navigator's environment (spec 0044, *Sidebar*).
@MainActor
struct SidebarSelectionTests {
    @Test func aSectionKeepsItsStoredValueAndAnEnvironmentHasItsOwn() {
        let id = FixtureEnvironments.erpID
        #expect(SidebarSelection.section(.catalog).rawValue == "catalog")
        #expect(SidebarSelection(rawValue: "catalog") == .section(.catalog))
        let stored = SidebarSelection.environment(id).rawValue
        #expect(stored == "environment:" + id.uuidString.lowercased())
        #expect(SidebarSelection(rawValue: stored) == .environment(id))
        #expect(SidebarSelection(rawValue: "environment:nope") == nil)
        #expect(SidebarSelection(rawValue: "unknown") == nil)
        #expect(SidebarSelection.environment(id).section == .environments)
    }

    @Test func theNavigatorOpensAnEnvironmentAndForgetsItElsewhere() {
        let navigator = ShellNavigator()
        let id = FixtureEnvironments.erpID
        navigator.openEnvironment(id)
        #expect(navigator.section == .environments)
        #expect(navigator.selection == .environment(id))
        navigator.show(.environments)
        #expect(navigator.selection == .section(.environments))
        navigator.selection = .environment(id)
        navigator.show(.catalog)
        #expect(navigator.environment == nil)
        #expect(ShellDestination.environment(id).section == .environments)
    }
}

/// The environment status table of `apps/macos/design.md`.
struct EnvironmentAppearanceTests {
    @Test func everyStatusHasItsLabelToneAndSymbol() {
        let rows: [(EnvironmentStatus, String, Tone, String)] = [
            (.ready, "Ready", .positive, "checkmark.circle.fill"),
            (.partiallyReady, "Partially Ready", .caution, "circle.lefthalf.filled"),
            (.down, "Down", .neutral, "circle"),
            (.unrecognized, "Unknown", .neutral, "questionmark.circle"),
        ]
        for (status, label, tone, symbol) in rows {
            let appearance = StatusAppearance.environment(status: status)
            #expect(appearance.label == label)
            #expect(appearance.tone == tone)
            #expect(appearance.symbol == symbol)
        }
        #expect(StatusAppearance.serviceNotCreated.symbol == "circle.dashed")
    }
}
