import Foundation
import StacklyClient
import Testing

@testable import StacklyUI

/// The rules of the Services section and the icon (spec 0032 AC-1, AC-3, AC-4): the order of
/// the groups, the limit of eight, and every row of the button table in its order.
struct MenuBarRowsTests {
    private func instance(
        _ name: String, _ lifecycle: InstanceLifecycle, id: UUID = UUID()
    ) throws -> InstanceSummary {
        try InstanceContractFixture.mixed()[0].testing(
            id: id, name: name, lifecycle: lifecycle)
    }

    private func button(
        _ lifecycle: InstanceLifecycle, connected: Bool = true, operating: Bool = false
    ) throws -> MenuBarRowButton {
        MenuBarRows.button(
            for: try instance("orders", lifecycle), isConnected: connected,
            hasOtherOperation: operating)
    }

    @Test func groupsComeInTheOrderFailedChangingRunningStopped() throws {
        let names: [(String, InstanceLifecycle)] = [
            ("s-stopped", .stopped), ("r-running", .running), ("u-unknown", .unknown),
            ("f-failed", .failed), ("i-init", .initializing), ("t-starting", .starting),
            ("g-stopping", .stopping), ("v-recovering", .recovering),
            ("w-uninit", .uninitialized),
        ]
        let ordered = MenuBarRows.ordered(try names.map { try instance($0.0, $0.1) })

        #expect(
            ordered.map(\.name) == [
                "f-failed",
                "g-stopping", "t-starting", "u-unknown", "v-recovering",
                "r-running",
                "i-init", "s-stopped", "w-uninit",
            ])
    }

    @Test func namesSortAsAPersonReadsThemAndTiesBreakByID() throws {
        let low = UUID(uuidString: "00000000-0000-7000-8000-000000000001") ?? UUID()
        let high = UUID(uuidString: "00000000-0000-7000-8000-000000000002") ?? UUID()
        let ordered = MenuBarRows.ordered([
            try instance("db10", .running), try instance("db2", .running),
            try instance("same", .running, id: high), try instance("same", .running, id: low),
        ])

        #expect(ordered.map(\.name) == ["db2", "db10", "same", "same"])
        #expect(ordered.suffix(2).map(\.id) == [low, high])
    }

    @Test func aRetainedInstanceIsInNoList() throws {
        #expect(MenuBarRows.ordered([try instance("gone", .retained)]).isEmpty)
    }

    @Test func theButtonTableRunsInTheOrderOfThePolicy() throws {
        // Offline wins over everything.
        let offline = try button(.running, connected: false, operating: true)
        #expect(offline.isEnabled == false)
        #expect(offline.tooltip == "stacklyd isn’t reachable.")
        // Another operation wins over the lifecycle rows.
        let busy = try button(.stopped, operating: true)
        #expect(busy.isEnabled == false)
        #expect(busy.tooltip == "Another operation is running for this instance.")
        for lifecycle in [InstanceLifecycle.uninitialized, .initializing] {
            let created = try button(lifecycle)
            #expect(created.isEnabled == false && created.tooltip == "Still being created.")
        }
        let unknown = try button(.unknown)
        #expect(unknown.isEnabled == false)
        #expect(
            unknown.tooltip == "Stackly can’t confirm this process. Open Stackly to check it.")
        let stopping = try button(.stopping)
        #expect(stopping.isEnabled == false && stopping.tooltip == "Stopping…")
    }

    @Test func aStoppedOrFailedInstanceStartsAndARunningOneStops() throws {
        for lifecycle in [InstanceLifecycle.stopped, .failed] {
            let start = try button(lifecycle)
            #expect(start.kind == .start && start.isEnabled && start.verb == .start)
            #expect(start.tooltip == "Start orders")
        }
        for lifecycle in [InstanceLifecycle.starting, .running, .recovering] {
            let stop = try button(lifecycle)
            #expect(stop.kind == .stop && stop.isEnabled && stop.verb == .stop)
            #expect(stop.tooltip == "Stop orders")
        }
    }

    @Test func theIconPutsOfflineBeforeFailedBeforeNormal() {
        #expect(MenuBarIconVariant.of(isConnected: true, failedCount: 0) == .normal)
        #expect(MenuBarIconVariant.of(isConnected: true, failedCount: 2) == .failed)
        #expect(MenuBarIconVariant.of(isConnected: false, failedCount: 2) == .offline)
        #expect(MenuBarIconVariant.of(isConnected: false, failedCount: 0) == .offline)
    }

    @MainActor @Test func everyVariantIsATemplateImage() {
        for variant in MenuBarIconVariant.allCases {
            #expect(MenuBarIcon.image(variant).isTemplate)
        }
    }

    @Test func aPercentComesFromCountedProgressOnly() {
        #expect(MenuBarModel.percent(of: .indeterminate) == nil)
        #expect(MenuBarModel.percent(of: .counted(CountedProgress(done: 1, total: 4))) == 25)
        #expect(MenuBarModel.percent(of: .counted(CountedProgress(done: 9, total: 4))) == 100)
        #expect(MenuBarModel.percent(of: .counted(CountedProgress(done: 0, total: 0))) == nil)
    }
}
