import Foundation
import StacklyClient
import StacklyTestSupport
import Testing

@testable import StacklyUI

/// The walking skeleton from the GUI side (spec 0014 AC-14 and AC-15; PRD AC-001): the
/// real `skeleton_seed` and `stacklyd` binaries, the live shell model, and a second
/// `StacklyClient` connection that makes the changes the model must follow.
///
/// Each test runs its own seeded root and daemon. Without the binaries the tests end as
/// BLOCKED (with evidence).
@Suite(.tags(.daemon), .timeLimit(.minutes(3)))
struct SkeletonEndToEndTests {
    /// The names of the three instances AC-15 keeps running.
    private static let names = ["alpha", "beta", "gamma"]

    // MARK: - Events (AC-14)

    @Test(
        .evidence(
            covers: ["AC-001", "AC-029"],
            expected:
                "the instance store follows an instance another client creates, starts, and stops, through events alone, and the GUI model sends no mutation"
        ))
    func theInstanceStoreFollowsChangesAnotherClientMakes() async throws {
        try await SkeletonRig.run(recordsEvidence: true) { rig in
            let parts = ShellEndToEndTests.shell(
                for: rig.daemon, announcements: ShellEndToEndTests.Announcements())
            let model = parts.runtime.model
            let trail = InstanceTrail()
            do {
                try await ShellEndToEndTests.waitUntilRefreshed(
                    model, "the shell to connect and refresh")
                #expect(model.instances.count == 0)
                trail.follow("demo", in: model.instances)

                let id = try await rig.create("demo", settings: ["readyDelayMs": .number(1500)])
                try await waitFor(
                    "demo to appear in the store", seconds: 20,
                    state: { "steps \(trail.steps)" },
                    until: { model.instances.instances.contains { $0.id == id } })
                try await waitFor(
                    "demo to be stopped in the store", seconds: 20,
                    state: { "steps \(trail.steps)" },
                    until: { trail.steps.contains { $0.lifecycle == .stopped } })

                try await rig.start(id)
                try await waitFor(
                    "demo to be ready in the store", seconds: 30,
                    state: { "steps \(trail.steps)" },
                    until: { trail.firstIndex(.running, .ready) != nil })
                let notReady = try #require(
                    trail.firstIndex(.running, .notReady), "steps \(trail.steps)")
                let ready = try #require(trail.firstIndex(.running, .ready))
                #expect(notReady < ready, "steps \(trail.steps)")

                try await rig.stop(id)
                try await waitFor(
                    "demo to end stopped in the store", seconds: 30,
                    state: { "steps \(trail.steps)" },
                    until: {
                        model.instances.instances.first { $0.id == id }?.lifecycle == .stopped
                            && model.instances.instances.first { $0.id == id }?.process == nil
                    })
                trail.finish()
                let last = try #require(trail.steps.last)
                #expect(last.lifecycle == .stopped, "steps \(trail.steps)")

                // The model made no mutation: every operation is one the second
                // connection started.
                try await Task.sleep(for: .milliseconds(300))
                let operations = try await rig.operationIDs()
                #expect(operations == Set(rig.startedOperations))
                Evidence.actual(
                    "demo went through \(trail.steps.map(\.description).joined(separator: ", ")) in the store; operation.list held only the \(operations.count) operations of the second connection"
                )
            } catch {
                trail.finish()
                await parts.shutDown()
                throw error
            }
            await parts.shutDown()
        }
    }

    // MARK: - Close and reopen (AC-15)

    @Test(
        .evidence(
            covers: ["AC-001"],
            expected:
                "after the GUI quits three ready instances keep running with the same PIDs and answer GET /health, no operation starts, and a new model holds all three"
        ))
    func quittingTheGUIKeepsThreeInstancesRunningForTheNextModel() async throws {
        try await SkeletonRig.run(recordsEvidence: true) { rig in
            var ids: [UUID] = []
            for name in Self.names {
                let id = try await rig.create(name)
                try await rig.start(id)
                _ = try await rig.waitForInstance(id, "\(name) to be ready") {
                    $0.lifecycle == .running && $0.readiness.state == .ready
                }
                ids.append(id)
            }
            let before = try await Self.runningProcesses(rig, ids)
            #expect(before.count == 3)

            let parts = ShellEndToEndTests.shell(
                for: rig.daemon, announcements: ShellEndToEndTests.Announcements())
            let first = parts.runtime
            try await ShellEndToEndTests.waitUntilRefreshed(
                first.model, "the shell to connect and refresh")
            try await waitFor("the first model to hold three instances", seconds: 20) {
                first.model.instances.count == 3
            }
            let operationsBefore = try await rig.operationIDs()

            first.willTerminate()
            await parts.shutDown()
            // Every wait before a new attempt passes; the daemon is not asked anything.
            try await Task.sleep(for: .seconds(1))

            let after = try await Self.runningProcesses(rig, ids)
            #expect(after == before)
            for id in ids {
                let current = try await rig.instance(id)
                #expect(try await SkeletonRig.healthStatus(of: current) == 200)
            }
            #expect(try await rig.operationIDs() == operationsBefore)
            #expect(rig.daemon.isRunning)

            let reopened = ShellBootstrap.live(
                rootOverride: rig.daemon.root, clientVersion: "0.1.0")
            defer { reopened.willTerminate() }
            try await ShellEndToEndTests.waitUntilRefreshed(
                reopened.model, "the new model to connect and refresh")
            try await waitFor("the new model to hold three instances", seconds: 20) {
                reopened.model.instances.count == 3
            }
            let held = Dictionary(
                uniqueKeysWithValues: reopened.model.instances.instances.map {
                    ($0.id, $0.process?.pid)
                })
            for id in ids {
                #expect(held[id] == before[id], "instance \(id)")
            }
            Evidence.actual(
                "alpha, beta, and gamma kept PIDs \(before.values.sorted().map(String.init).joined(separator: ", ")), answered GET /health with 200 after the quit, and a new model held all three"
            )
        }
    }

    /// The PID of each instance, after checking that it runs and is ready.
    private static func runningProcesses(
        _ rig: SkeletonRig, _ ids: [UUID]
    ) async throws -> [UUID: UInt32] {
        var pids: [UUID: UInt32] = [:]
        for id in ids {
            let current = try await rig.instance(id)
            #expect(current.lifecycle == .running, "instance \(current.name)")
            #expect(current.readiness.state == .ready, "instance \(current.name)")
            pids[id] = current.process?.pid
        }
        return pids
    }
}
