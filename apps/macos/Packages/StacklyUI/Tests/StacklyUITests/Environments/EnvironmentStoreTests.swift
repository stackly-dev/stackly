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
                "an environment operation that ended loads env.list again; one that runs or another resource does not"
        ))
    func anEndedEnvironmentOperationLoadsTheListAgain() async throws {
        let store = store()
        await store.store.resnapshot()
        store.store.receive(event(operation(.environment, state: .running)))
        store.store.receive(event(operation(.instance, state: .succeeded), sequence: 2))
        for _ in 0..<10 { await Task.yield() }
        #expect(script.listCalls == 1)
        store.store.receive(event(operation(.environment, state: .succeeded), sequence: 3))
        try await waitUntil { script.listCalls == 2 }
        #expect(store.changes >= 2)
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
