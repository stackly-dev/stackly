import Foundation
import StacklyClient
import Testing

@testable import StacklyUI

/// Restart, Reload, and Force Stop… of an instance (spec 0026 AC-14, AC-15, AC-18, *Alasan
/// nonaktif*): every row of the reason table, in its order.
struct InstanceLifecycleActionsTests {
    private func resolve(
        _ lifecycle: InstanceLifecycle, reload: Bool = true, local: InstanceLocalStatus = .none
    ) -> InstanceLifecycleActions {
        let summary = InstanceSummary.fixture(
            lifecycle: lifecycle, capabilities: InstanceCapabilities(reload: reload))
        return InstanceLifecycleActions.resolve(
            summary: summary, local: local, providerName: "NATS")
    }

    @Test func aRunningInstanceCanRestartAndReloadButNotForceStop() {
        let actions = resolve(.running)
        #expect(actions.restart.disabledReason == nil)
        #expect(actions.reload.disabledReason == nil)
        #expect(
            actions.forceStop.disabledReason
                == "Force stop is available while an instance is still stopping.")
        #expect(actions.restart.symbol == "arrow.clockwise")
        #expect(actions.reload.symbol == "bolt")
    }

    @Test func aRecoveringInstanceCanRestartButNotReload() {
        let actions = resolve(.recovering)
        #expect(actions.restart.disabledReason == nil)
        #expect(actions.reload.disabledReason == "Reload needs a running instance.")
    }

    @Test func aStoppedInstanceNeedsToRunForRestartAndReload() {
        let actions = resolve(.stopped)
        #expect(actions.restart.disabledReason == "Restart needs a running instance.")
        #expect(actions.reload.disabledReason == "Reload needs a running instance.")
    }

    @Test func aProviderWithoutReloadNamesItselfAndSuggestsRestartFirst() {
        // The capability comes before the lifecycle, so a stopped NATS says so too.
        for lifecycle in [InstanceLifecycle.running, .stopped, .unknown] {
            #expect(
                resolve(lifecycle, reload: false).reload.disabledReason
                    == "NATS doesn’t support reload. Restart to apply configuration changes.")
        }
    }

    @Test func anUnknownInstanceCannotBeConfirmedForAnyAction() {
        let actions = resolve(.unknown)
        let reason = "Stackly can’t confirm this instance’s process."
        #expect(actions.restart.disabledReason == reason)
        #expect(actions.reload.disabledReason == reason)
        #expect(actions.forceStop.disabledReason == reason)
    }

    @Test func onlyAStoppingInstanceCanForceStop() {
        let actions = resolve(.stopping)
        #expect(actions.forceStop.disabledReason == nil)
        #expect(actions.forceStop.title == "Force Stop…")
        #expect(actions.restart.disabledReason == "Restart needs a running instance.")
    }

    @Test func aClickInFlightShowsItsBusyLabelAndMakesTheOthersBusy() {
        let actions = resolve(.running, local: .inFlight(.restart))
        #expect(actions.restart.title == "Restarting…")
        #expect(actions.restart.isBusy)
        #expect(actions.reload.isBusy)
        #expect(actions.reload.title == "Reload")
        #expect(resolve(.running, local: .inFlight(.reload)).reload.title == "Reloading…")
        #expect(
            resolve(.stopping, local: .inFlight(.forceStop)).forceStop.title == "Force stopping…")
    }

    @Test func aSuccessfulRestartReloadAndForceStopShowTheCopyOfTheSpec() {
        #expect(
            InstanceActionModel.success(verb: .restart, name: "pg", leftover: [])?.message
                == "Restarted “pg”.")
        #expect(
            InstanceActionModel.success(verb: .reload, name: "pg", leftover: [])?.message
                == "Reloaded “pg”.")
        #expect(
            InstanceActionModel.success(verb: .forceStop, name: "pg", leftover: [])?.message
                == "Force stopped “pg”.")
        #expect(
            InstanceActionModel.success(verb: .forceStop, name: "pg", leftover: [41, 42])?.message
                == "Force stopped “pg”. Still running: PID 41, 42.")
        #expect(InstanceActionModel.success(verb: .start, name: "pg", leftover: []) == nil)
    }

    @Test func aRestartThatStoppedHalfwaySaysTheInstanceStaysStopped() {
        let body = ErrorBody(
            code: "supervisor.stop_timeout", category: .timeout,
            message: "the instance did not stop within 10 s", retryable: false, resource: nil,
            fields: nil, hint: nil,
            details: [
                ErrorDetail(key: "timeoutSeconds", value: "10"),
                ErrorDetail(key: "after", value: "stays_stopped"),
            ], operationId: nil, rule: nil, items: nil)
        let card = InstanceActionModel.failure(verb: .restart, name: "pg", body: body)
        #expect(card.message == "Couldn’t restart “pg”.")
        #expect(
            card.detail
                == "The restart stopped halfway. “pg” stays stopped; use Start when it has finished stopping."
        )
    }

    @Test func lastExitShowsOnlyForcedAndCrashed() {
        #expect(DetailFormat.lastExit(.forced) == "Forced")
        #expect(DetailFormat.lastExit(.crashed) == "Crashed")
        #expect(DetailFormat.lastExit(.clean) == nil)
        #expect(DetailFormat.lastExit(nil) == nil)
        #expect(DetailFormat.lastExit(.unrecognized) == nil)
    }

    @Test func theStillStoppingBannerShowsOnlyWhileStoppingWithoutALocalClick() {
        let stopping = InstanceSummary.fixture(lifecycle: .stopping)
        let force = StacklyAction(
            id: "forceStop", title: "Force Stop…",
            role: .destructive(
                ActionConfirmation(
                    title: "Force stop “pg”?", message: "m", actionTitle: "Force Stop")),
            mutates: true, perform: {})
        #expect(
            InstanceDetailPage.stoppingActions(
                summary: stopping, local: .none, stopAgain: {}, forceStop: force) != nil)
        #expect(
            InstanceDetailPage.stoppingActions(
                summary: stopping, local: .inFlight(.stop), stopAgain: {}, forceStop: force)
                == nil)
        #expect(
            InstanceDetailPage.stoppingActions(
                summary: .fixture(lifecycle: .running), local: .none, stopAgain: {},
                forceStop: force) == nil)
    }
}
