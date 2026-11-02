import Foundation
import StacklyClient
import Testing

@testable import StacklyUI

/// The banner of a daemon restart as a pure value of the plan (spec 0030 AC-16): the
/// restarting banner while the daemon is away, the summary while it brings the instances
/// back, and a warning when the plan did not go through.
struct RestartBannerContentTests {
    private static let now = FixtureClock.now
    private typealias F = FixtureRestart

    private static func banner(
        _ plan: RestartPlanView?, awaiting: Bool = false, dismissed: UUID? = nil,
        at now: Date = now
    ) -> RestartBannerContent? {
        RestartBannerContent.make(
            plan: plan, isAwaitingPlan: awaiting, dismissedPlanID: dismissed, now: now)
    }

    @Test func aRequestedRestartShowsTheBannerBeforeThePlanArrives() throws {
        let banner = try #require(Self.banner(nil, awaiting: true))
        #expect(banner.title == "stacklyd is restarting")
        #expect(banner.tone == .informative)
        #expect(Self.banner(nil) == nil)
    }

    @Test func whileTheDaemonIsAwayTheBannerCountsWhatKeepsRunningAndWhatStops() throws {
        for state in [PlanState.stopping, .handedOver] {
            let banner = try #require(Self.banner(F.plan(state, instances: F.mixedInstances)))
            #expect(banner.title == "stacklyd is restarting")
            #expect(
                banner.message == "2 instances keep running. 1 instance stops and starts again.")
            #expect(banner.canDismiss == false)
        }
    }

    @Test func theMessageNamesTheCountsInTheRightNumber() throws {
        let one = [F.instance(1, "a", decision: .stopRestore, reason: .handoverUnproven)]
        #expect(
            try #require(Self.banner(F.plan(.stopping, instances: one))).message
                == "1 instance stops and starts again.")
        let kept = [F.instance(1, "a"), F.instance(2, "b")]
        #expect(
            try #require(Self.banner(F.plan(.stopping, instances: kept))).message
                == "2 instances keep running.")
        #expect(
            try #require(Self.banner(F.plan(.stopping, instances: []))).message
                == "No instance is affected.")
    }

    @Test func whileTheNewDaemonRestoresTheSummaryCountsTheOutcomesAndTheSpans() throws {
        let instances = [
            F.instance(1, "pg17-dev", outcome: .adopted),
            F.instance(
                3, "valkey-cache", decision: .stopRestore, reason: .handoverUnproven,
                stopped: true),
        ]
        let banner = try #require(Self.banner(F.plan(.restoring, instances: instances)))
        #expect(banner.title == "stacklyd restarted")
        #expect(banner.tone == .informative)
        #expect(
            banner.message
                == "1 adopted, 0 started again, 0 failed, 1 still starting. Stopping 3.0 s, daemon away 2.0 s."
        )
    }

    @Test func aFinishedRestartWithoutFailuresLeavesOnlyTheToast() {
        let instances = [
            F.instance(1, "pg17-dev", outcome: .adopted),
            F.instance(3, "valkey-cache", decision: .stopRestore, outcome: .restored),
        ]
        #expect(Self.banner(F.plan(.finished, instances: instances)) == nil)
    }

    @Test func aFinishedRestartWithAFailureWarnsAndListsTheInstance() throws {
        let instances = [
            F.instance(1, "pg17-dev", outcome: .adopted),
            F.instance(
                3, "valkey-cache", decision: .stopRestore, outcome: .failed,
                failureCode: "endpoint_in_use"),
        ]
        let banner = try #require(Self.banner(F.plan(.finished, instances: instances)))
        #expect(banner.tone == .caution)
        #expect(banner.title == "stacklyd restarted, but 1 instance did not come back")
        #expect(
            banner.items == [
                .init(name: "valkey-cache", reason: "Did not come back (endpoint_in_use).")
            ])
        #expect(banner.canDismiss)
        #expect(
            banner.message
                == "1 adopted, 0 started again, 1 failed. Stopping 3.0 s, daemon away 2.0 s, restoring 4.0 s."
        )
    }

    @Test func anAbortedRestartNamesTheReasonAndTheInstanceThatDidNotStop() throws {
        let instances = [
            F.instance(1, "pg17-dev"),
            F.instance(3, "valkey-cache", decision: .stopRestore, reason: .handoverUnproven),
        ]
        let plan = F.plan(.aborted, instances: instances, closeReason: .stopTimeout)
        let banner = try #require(Self.banner(plan))
        #expect(banner.tone == .caution)
        #expect(banner.title == "Daemon restart cancelled")
        #expect(banner.message.hasPrefix("An instance did not stop in time."))
        #expect(banner.items == [.init(name: "valkey-cache", reason: "Did not stop in time.")])
    }

    @Test func anExpiredRestartSaysTheMacStartedAgain() throws {
        let plan = F.plan(.expired, instances: F.mixedInstances, closeReason: .bootChanged)
        let banner = try #require(Self.banner(plan))
        #expect(banner.title == "Daemon restart expired")
        #expect(banner.message.hasPrefix("The Mac started again before the restart finished."))
    }

    @Test func aWarningLeavesWhenDismissedOrAfterTheSummaryTime() throws {
        let plan = F.plan(.expired, instances: [], closeReason: .bootChanged)
        #expect(Self.banner(plan, dismissed: plan.id) == nil)
        let finished = try #require(plan.finishedAt)
        #expect(Self.banner(plan, at: finished.addingTimeInterval(599)) != nil)
        #expect(Self.banner(plan, at: finished.addingTimeInterval(600)) == nil)
    }

    @Test func theCountsTallyDecisionsAndOutcomes() {
        let plan = F.plan(
            .restoring,
            instances: [
                F.instance(1, "a", outcome: .adopted),
                F.instance(2, "b", decision: .stopRestore, outcome: .restored),
                F.instance(3, "c", decision: .stopRestore, outcome: .failed),
                F.instance(4, "d", outcome: .skipped),
                F.instance(5, "e", decision: .stopRestore),
            ])
        let counts = RestartBannerContent.counts(of: plan)
        #expect(counts.adopted == 1)
        #expect(counts.restored == 1)
        #expect(counts.failed == 1)
        #expect(counts.skipped == 1)
        #expect(counts.pending == 1)
        #expect(counts.keeping == 2)
        #expect(counts.stopping == 3)
    }
}
