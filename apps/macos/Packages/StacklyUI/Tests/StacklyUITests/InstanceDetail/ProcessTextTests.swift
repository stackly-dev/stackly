import Foundation
import StacklyClient
import Testing

@testable import StacklyUI

/// The Process row (spec 0021, *Format bersama*): one word or phrase per lifecycle state,
/// apart from the Readiness row.
struct ProcessTextTests {
    @Test func aRunningProcessShowsItsPidAndWhetherItWasAdopted() throws {
        let running = try DetailFixture.triple("postgresql").instance
        #expect(ProcessText.text(for: running) == "Running (PID 4001)")

        let process = try #require(running.process)
        let adopted = InstanceProcess(
            pid: process.pid, startedAt: process.startedAt, adopted: true, adoptable: true)
        let found = InstanceSummary(
            id: running.id, name: running.name, displayName: running.displayName,
            revision: running.revision, software: running.software, provider: running.provider,
            runtimeId: running.runtimeId, runtimeVersion: running.runtimeVersion,
            lifecycle: .running, desired: running.desired, readiness: running.readiness,
            health: running.health, statusSeq: running.statusSeq, process: adopted,
            lastFailure: nil, restart: running.restart, blocker: nil,
            endpoints: running.endpoints, leftoverProcesses: running.leftoverProcesses,
            logsUnavailable: false, ownership: running.ownership, initialized: true)
        #expect(ProcessText.text(for: found) == "Running (PID 4001, adopted)")
    }

    @Test func everyOtherLifecycleStateHasItsWord() throws {
        let base = try DetailFixture.triple("postgresql").instance
        let words: [(InstanceLifecycle, String)] = [
            (.recovering, "Restarting"), (.starting, "Starting"), (.stopping, "Stopping"),
            (.stopped, "Stopped"), (.failed, "Failed"), (.unknown, "Unknown"),
            (.initializing, "Initializing"), (.uninitialized, "Not initialized"),
            (.retained, "Removed"),
        ]
        for (lifecycle, word) in words {
            #expect(ProcessText.text(for: base.testing(lifecycle: lifecycle)) == word)
        }
    }

    @Test func theProcessAndTheReadinessAreTwoSeparateTexts() throws {
        let running = try DetailFixture.triple("postgresql").instance
        let waiting = running.testing(
            readiness: InstanceReadiness(
                state: .notReady, reason: "awaiting_probe", detail: nil,
                observedAt: running.readiness.observedAt))
        #expect(ProcessText.text(for: waiting) == "Running (PID 4001)")
        #expect(ReadinessText.text(for: waiting) == "Waiting for probe")
    }
}
