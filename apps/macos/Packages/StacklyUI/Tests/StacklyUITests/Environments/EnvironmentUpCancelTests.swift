import Foundation
import StacklyClient
import StacklyTestSupport
import Testing

@testable import StacklyUI

/// The plan footnote of Up and the rule the "Bringing up…" banner offers Cancel by
/// (spec 0046, AC-19, AC-20): Cancel comes only from `OperationSummary.cancel`.
struct EnvironmentUpCancelTests {
    /// `operation` with `cancel` in place of its own.
    private static func with(_ operation: OperationSummary, cancel: CancelAvailability?)
        -> OperationSummary
    {
        OperationSummary(
            id: operation.id, kind: operation.kind, resource: operation.resource,
            state: operation.state, stage: operation.stage, progress: operation.progress,
            waitingOn: operation.waitingOn, cancelRequestedAt: operation.cancelRequestedAt,
            pointOfNoReturnAt: operation.pointOfNoReturnAt, failure: operation.failure,
            recovery: operation.recovery, result: operation.result, trigger: operation.trigger,
            createdAt: operation.createdAt, startedAt: operation.startedAt,
            updatedAt: operation.updatedAt, revision: operation.revision, cancel: cancel,
            parent: operation.parent)
    }

    @Test func thePlanFootnoteSaysServicesStartInDependencyOrderAndTheRestStayUp() {
        #expect(
            EnvironmentCopy.planFootnote
                == "Services start in dependency order and wait for readiness. If one fails, the rest stay up and the environment is marked partially ready."
        )
    }

    @Test func aRunningUpOffersCancelOnlyWhileTheDaemonSaysItIsAvailable() {
        let running = FixtureEnvironmentUp.upOperation()
        #expect(running.state == .running)
        #expect(OperationCancel.availability(of: running) == .available)

        let refused: [CancelAvailability] = [.pastPointOfNoReturn, .requested, .continuing, .none]
        for cancel in refused {
            let summary = Self.with(running, cancel: cancel)
            #expect(
                OperationCancel.availability(of: summary) != .available,
                "a running Up whose cancel is \(cancel) offers no Cancel")
        }
    }

    @Test func anUpThatEndedOffersNoCancel() {
        let ended = FixtureEnvironmentUp.upOperation(
            state: .failed, done: 4, total: 4,
            failure: FixtureEnvironmentUp.incompleteFailure(ready: 2, total: 4))
        #expect(OperationCancel.availability(of: ended) == CancelAvailability.none)
        #expect(OperationCancel.availability(of: Self.with(ended, cancel: nil)) == .none)
    }
}
