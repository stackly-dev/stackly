import AppKit
import Foundation
import StacklyClient
import StacklyTestSupport
import SwiftUI
import Testing

@testable import StacklyUI

/// The first snapshot with 100 instances (spec 0022 AC-14, NFR-002): the app takes the
/// list from a scripted daemon in at most 1 second after the handshake, and the Overview
/// groups and sorts the instances again only when they change.
@Suite(.timeLimit(.minutes(2)))
@MainActor
struct InitialSnapshotTests {
    private static let instanceCount = 100

    /// 100 stopped instances of the fixture providers, each with its own name and ID.
    static func hundredInstances() -> [InstanceSummary] {
        (1...instanceCount).map { number in
            .fixture(
                number: UInt8(number), name: "perf-\(String(format: "%03d", number))",
                port: UInt16(20_000 + number))
        }
    }

    /// The bound a timing is checked against: the spec's own in an evidence run, and ten
    /// times looser in a plain `swift test`, where other work on this Mac stretches it.
    private static func bound(_ spec: Duration) -> Duration {
        ProcessInfo.processInfo.environment["STACKLY_EVIDENCE_DIR"] == nil ? spec * 10 : spec
    }

    @Test(
        .evidence(
            covers: ["NFR-002"],
            expected:
                "from the accepted handshake to InstanceStore.loadedAt the app takes at most 1 second for a scripted daemon that answers 100 instances"
        ))
    func theAppHoldsTheHundredInstancesOneSecondAfterTheHandshake() async throws {
        let daemon = try ScriptedDaemon(instances: Self.hundredInstances())
        daemon.start()
        let parts = ShellBootstrap.assemble(
            rootOverride: daemon.rootOverride, clientVersion: "0.1.0",
            closeNotice: CloseNotice(presentsPanel: false))
        let store = parts.runtime.model.instances.store

        try await waitFor("the instance store to load", seconds: 30) { store.loadedAt != nil }
        let handshake = try #require(daemon.acceptedHandshakeAt)
        let loaded = try #require(store.loadedAt)
        let elapsed = Duration.seconds(loaded.timeIntervalSince(handshake))

        #expect(store.count == Self.instanceCount)
        let millis = Int(loaded.timeIntervalSince(handshake) * 1000)
        Evidence.actual(
            "\(store.count) instances in the store \(millis) ms after the handshake was accepted")
        #expect(
            elapsed <= Self.bound(.seconds(1)),
            "the store took \(millis) ms after the handshake")
        await parts.shutDown()
    }

    // MARK: - Recompute

    /// The Overview of `rig`, as the shell builds it.
    private func page(
        _ rig: PageRig, _ derivation: OverviewDerivation, isConnected: Bool = true
    ) -> AnyView {
        AnyView(
            rig.overviewPage(isConnected: isConnected, derivation: derivation)
                .environment(\.fixtureNow, FixtureClock.now))
    }

    /// Shows the Overview of `rig` in an off screen window and returns the window and the
    /// hosting view, so the body runs for real and a test can give it new parameters.
    private func mount(_ rig: PageRig, _ derivation: OverviewDerivation) -> (
        window: NSWindow, hosting: NSHostingView<AnyView>
    ) {
        let hosting = NSHostingView(rootView: page(rig, derivation))
        let window = SnapshotWindow(size: CGSize(width: 1_044, height: 748))
        window.contentView = hosting
        hosting.frame = CGRect(x: 0, y: 0, width: 1_044, height: 748)
        hosting.layoutSubtreeIfNeeded()
        return (window, hosting)
    }

    /// Lets the page run what is pending: the loads, the observations, and the layout.
    private func settle(_ window: NSWindow) async throws {
        for _ in 0..<10 {
            window.contentView?.layoutSubtreeIfNeeded()
            try await Task.sleep(for: .milliseconds(50))
        }
    }

    @Test(
        .evidence(
            covers: ["NFR-002"],
            expected:
                "an operation event or a new connection state that changes no instance does not make the Overview derive its instances again; an instance event does"
        ))
    func anEventOrStateThatChangesNoInstanceDoesNotMakeTheOverviewDeriveThemAgain() async throws {
        let rig = PageRig(instances: Self.hundredInstances(), operations: [], capacity: nil)
        await rig.load()
        let derivation = OverviewDerivation()
        let (window, hosting) = mount(rig, derivation)
        defer { window.close() }
        try await settle(window)
        let settled = derivation.recomputes
        #expect(settled >= 1, "the page derived its instances once")

        // The daemon knows these operations now: an event makes the store read each one.
        let running = (1...20).map {
            OperationSummary.fixture(number: UInt8($0), kind: .startInstance, state: .running)
        }
        rig.script.operations = running
        for (offset, operation) in running.enumerated() {
            let sequence = offset + 1
            rig.operations.store.receive(
                Event(
                    subscriptionId: 1, sequence: UInt64(sequence), at: FixtureClock.now,
                    resource: ResourceRef(
                        kind: .operation, id: operation.id.uuidString.lowercased()),
                    revision: UInt64(sequence), kind: .operationProgress(operation)))
        }
        try await waitUntil { rig.operations.active.count == 20 }
        try await settle(window)
        #expect(derivation.recomputes == settled, "operation events derive nothing again")

        // The page runs its body again for a reason that is none of the instances.
        hosting.rootView = page(rig, derivation, isConnected: false)
        try await settle(window)
        #expect(derivation.recomputes == settled, "a new connection state derives nothing")

        let changed = Self.hundredInstances()[0]
        rig.instances.store.receive(
            Event(
                subscriptionId: 1, sequence: 100, at: FixtureClock.now,
                resource: ResourceRef(kind: .instance, id: changed.id.uuidString.lowercased()),
                revision: changed.revision,
                kind: .instanceHealthChanged(
                    InstanceHealthChanged(
                        instanceId: changed.id, statusSeq: 50,
                        readiness: .fixture(.ready, reason: "ready"), health: changed.health,
                        blocker: nil, restart: changed.restart, process: nil,
                        logsUnavailable: false))))
        try await waitUntil {
            rig.instances.instances.first { $0.id == changed.id }?.readiness.state == .ready
        }
        try await settle(window)
        #expect(derivation.recomputes == settled + 1, "an instance event derives once")
        Evidence.actual(
            "20 operation events left the recompute counter at \(settled); one instance event raised it to \(derivation.recomputes)"
        )
    }
}
