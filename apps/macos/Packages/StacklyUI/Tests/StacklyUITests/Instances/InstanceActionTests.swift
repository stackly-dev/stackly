import Foundation
import StacklyClient
import Testing

@testable import StacklyUI

/// The main action of an instance (spec 0022 AC-8, *Aksi instance*): every row of the table.
struct InstanceActionTests {
    private func action(
        _ lifecycle: InstanceLifecycle, initialized: Bool = true,
        local: InstanceLocalStatus = .none
    ) -> InstanceAction {
        let base = InstanceSummary.fixture(lifecycle: lifecycle)
        let summary = InstanceSummary(
            id: base.id, name: base.name, displayName: nil, revision: base.revision,
            software: base.software, provider: base.provider, runtimeId: base.runtimeId,
            runtimeVersion: base.runtimeVersion, lifecycle: lifecycle, desired: base.desired,
            readiness: base.readiness, health: base.health, statusSeq: base.statusSeq,
            process: nil, lastFailure: nil, restart: base.restart, blocker: nil,
            endpoints: base.endpoints, leftoverProcesses: nil, logsUnavailable: false,
            ownership: .manual, initialized: initialized)
        return InstanceAction.resolve(summary: summary, local: local)
    }

    @Test func aStoppedInstanceOffersStart() {
        let start = action(.stopped)
        #expect(start.kind == .start)
        #expect(start.title == "Start")
        #expect(start.verb == .start)
        #expect(start.disabledReason == nil)
        #expect(start.commandVerb == .start)
    }

    @Test func aFailedInstanceOffersRetryAndStartsAgain() {
        let retry = action(.failed)
        #expect(retry.kind == .retry)
        #expect(retry.title == "Retry")
        #expect(retry.verb == .start)
        #expect(retry.disabledReason == nil)
    }

    @Test func aFailedInstanceThatWasNeverInitializedCannotRetry() {
        let retry = action(.failed, initialized: false)
        #expect(retry.kind == .retry)
        #expect(retry.verb == nil)
        #expect(
            retry.disabledReason
                == "Initialization failed. Remove this instance and create it again.")
        #expect(retry.commandVerb == .start)
    }

    @Test func aRunningOrRecoveringInstanceOffersStop() {
        for lifecycle in [InstanceLifecycle.running, .recovering] {
            let stop = action(lifecycle)
            #expect(stop.kind == .stop, "\(lifecycle)")
            #expect(stop.verb == .stop)
            #expect(stop.disabledReason == nil)
        }
    }

    @Test func aChangeThatTheDaemonMakesShowsABusyButton() {
        let initializing = action(.initializing)
        #expect(initializing.title == "Initializing…")
        #expect(initializing.isBusy)
        #expect(initializing.verb == nil)
        #expect(initializing.commandVerb == .start)
        #expect(action(.starting).title == "Starting…")
        #expect(action(.starting).commandVerb == .start)
        #expect(action(.stopping).title == "Stopping…")
        #expect(action(.stopping).commandVerb == .stop)
    }

    @Test func anUnknownInstanceCannotBeStoppedAndSaysWhy() {
        let stop = action(.unknown)
        #expect(stop.kind == .stop)
        #expect(stop.verb == nil)
        #expect(stop.disabledReason == "Stackly can’t confirm this process.")
    }

    @Test func anUninitializedInstanceCannotBeStartedAndSaysWhy() {
        let start = action(.uninitialized)
        #expect(start.kind == .start)
        #expect(start.verb == nil)
        #expect(start.disabledReason == "This instance isn’t initialized.")
    }

    @Test func aClickInFlightOrPendingMakesTheButtonBusyWhateverTheLifecycle() {
        let inFlight = action(.stopped, local: .inFlight(.start))
        #expect(inFlight.isBusy)
        #expect(inFlight.title == "Starting…")
        #expect(inFlight.verb == nil)
        let pending = action(.running, local: .pending(.stop, operationId: UUID()))
        #expect(pending.isBusy)
        #expect(pending.title == "Stopping…")
        #expect(pending.commandVerb == .stop)
    }
}
