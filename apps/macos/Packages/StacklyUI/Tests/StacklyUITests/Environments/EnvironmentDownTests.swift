import Foundation
import StacklyClient
import StacklyTestSupport
import Testing

@testable import StacklyUI

/// Down of an environment in the app (spec 0047, AC-20, AC-21).
@MainActor
struct EnvironmentDownModelTests {
    let rig = PageRig(environments: FixtureEnvironments.summaries)

    private func model() -> (EnvironmentDownModel, Reports) {
        let model = EnvironmentDownModel(store: rig.environments, newKey: { "key-1" })
        let reports = Reports()
        model.report = { reports.items.append($0) }
        return (model, reports)
    }

    /// The feedback a model showed, in order.
    @MainActor
    final class Reports {
        var items: [Feedback] = []
    }

    private static func error(_ code: String) -> ErrorBody {
        ErrorBody(
            code: code, category: .conflict, message: code, retryable: false, resource: nil,
            fields: nil, hint: nil, details: nil, operationId: nil, rule: nil, items: nil)
    }

    @Test(
        .evidence(
            covers: ["REQ-022", "NFR-012"],
            expected:
                "Down reads env.preview_down and opens Bring down “erp-development”? with what stops, what keeps running and why, and Data is kept, with Cancel and Bring Down"
        ))
    func downOpensTheConfirmationOfThePreview() async throws {
        let (model, reports) = model()
        await model.begin(environment: FixtureEnvironments.erpID)
        let request = try #require(model.confirmation)
        #expect(request.title == "Bring down “erp-development”?")
        #expect(request.actionTitle == "Bring Down")
        #expect(request.role == .standard)
        #expect(request.copyProblems.isEmpty)
        #expect(
            request.message.components(separatedBy: "\n") == [
                "Releases the lease and stops only instances nothing else needs. Shared and manually started instances keep running. Data is kept.",
                "Stops: cache.",
                "Keeps running: pg17-dev (used by reporting), queue (started by you).",
            ])
        #expect(reports.items.isEmpty)
        #expect(rig.script.environmentSent.isEmpty)
        #expect(model.isBusy(FixtureEnvironments.erpID) == false)
    }

    @Test func theConfirmationNamesWhatCantStopAndTheUpItCancels() {
        let message = EnvironmentCopy.downMessage(
            FixtureEnvironmentDown.preview(cancelsUp: true, blocked: true))
        let lines = message.components(separatedBy: "\n")
        #expect(lines.contains("Can’t stop: legacy-pg (Stackly can’t confirm this process)."))
        #expect(lines.last == "This also cancels the Up in progress.")
    }

    @Test func aPreviewWithNothingToDoOpensNoConfirmationAndSendsNothing() async {
        rig.script.downPreviews = [FixtureEnvironmentDown.emptyPreview()]
        let (model, reports) = model()
        await model.begin(environment: FixtureEnvironments.erpID)
        #expect(model.confirmation == nil)
        #expect(reports.items.map(\.message) == ["Nothing to bring down."])
        #expect(rig.script.environmentSent.isEmpty)
    }

    @Test func aPreviewWithOnlyALeaseStillAsks() {
        let preview = FixtureEnvironmentDown.emptyPreview()
        #expect(EnvironmentDownModel.hasWork(preview) == false)
        let leased = EnvDownPreview(
            environment: preview.environment, stop: [], keep: [], idle: [], blocked: [],
            leaseCount: 1, cancelsUp: false)
        #expect(EnvironmentDownModel.hasWork(leased))
        #expect(
            EnvironmentCopy.downMessage(leased).components(separatedBy: "\n")[1]
                == "Nothing needs to stop.")
    }

    @Test(
        .evidence(
            covers: ["REQ-022"],
            expected:
                "Bring Down sends env.down with the IDs of the stop list the confirmation showed and watches the operation"
        ))
    func bringDownSendsTheIDsTheConfirmationShowed() async throws {
        let (model, _) = model()
        let preview = FixtureEnvironmentDown.preview()
        await model.bringDown(preview)
        guard case .envDown(let params) = try #require(rig.script.environmentSent.first) else {
            Issue.record("expected env.down")
            return
        }
        #expect(params.id == FixtureEnvironments.erpID)
        #expect(params.expectedStopInstanceIds == preview.stop.map(\.instanceId))
        #expect(
            model.watched[FixtureEnvironmentDown.downID]
                == EnvironmentDownModel.Watch(
                    environment: FixtureEnvironments.erpID, name: "erp-development", kept: 2,
                    keptLine: "Keeps running: pg17-dev (used by reporting), queue (started by you)."
                ))
    }

    @Test func aChangedSetOpensTheConfirmationAgainWithTheNewPreviewAndANote() async throws {
        let fresh = FixtureEnvironmentDown.preview(cancelsUp: true)
        rig.script.downPreviews = [fresh]
        rig.script.downOutcomes = [.rejected(Self.error("env.down_changed"))]
        let (model, reports) = model()
        await model.bringDown(FixtureEnvironmentDown.preview())
        let request = try #require(model.confirmation)
        let lines = request.message.components(separatedBy: "\n")
        #expect(lines.first == "What Down will stop changed. Review it again.")
        #expect(lines.last == "This also cancels the Up in progress.")
        #expect(reports.items.isEmpty)
        #expect(rig.script.environmentSent.count == 1)
    }

    @Test func aRefusalShowsTheDaemonsReason() async {
        rig.script.downOutcomes = [.rejected(Self.error("operation.queue_full"))]
        let (model, reports) = model()
        await model.bringDown(FixtureEnvironmentDown.preview())
        #expect(reports.items.map(\.code) == ["operation.queue_full"])
        #expect(model.watched.isEmpty)
    }

    @Test(
        .evidence(
            covers: ["REQ-022", "NFR-012"],
            expected:
                "When the Down the app sent ends, its result shows as feedback: erp-development is down. 1 stopped, 2 kept running."
        ))
    func theResultOfAWatchedDownShowsWhenItEnds() async {
        rig.script.operations = [
            FixtureEnvironmentDown.downOperation(
                state: .succeeded, done: 1, total: 1,
                result: FixtureEnvironmentDown.result(stopped: 1))
        ]
        let (model, reports) = model()
        await model.bringDown(FixtureEnvironmentDown.preview())
        await model.ended(
            EndedOperation(operation: UUID(), environment: FixtureEnvironments.erpID))
        #expect(reports.items.isEmpty)
        await model.ended(
            EndedOperation(
                operation: FixtureEnvironmentDown.downID, environment: FixtureEnvironments.erpID))
        #expect(
            reports.items.map(\.message) == [
                "erp-development is down. 1 stopped, 2 kept running."
            ])
        #expect(
            reports.items.first?.detail
                == "Keeps running: pg17-dev (used by reporting), queue (started by you). Shared and manually started instances keep running."
        )
        #expect(model.watched.isEmpty)
    }

    // MARK: - Feedback

    private static func feedback(_ operation: OperationSummary, kept: Int = 0) -> Feedback? {
        EnvironmentDownModel.feedback(
            operation, name: "erp-development", kept: kept, bringDownAgain: {},
            showOperation: {})
    }

    @Test func eachEndOfADownHasItsSentence() throws {
        let down = FixtureEnvironmentDown.self
        #expect(
            Self.feedback(
                down.downOperation(state: .succeeded, result: down.result(stopped: 2)))?.message
                == "erp-development is down. 2 instances stopped. Data is kept.")
        #expect(
            Self.feedback(
                down.downOperation(state: .succeeded, result: down.result(stopped: 1, inUse: 1)))?
                .message == "erp-development is down. 1 stopped, 1 kept running.")
        #expect(
            Self.feedback(
                down.downOperation(state: .succeeded, result: down.result(stopped: 1, inUse: 1)))?
                .detail == "Shared and manually started instances keep running.")
        #expect(
            Self.feedback(down.downOperation(state: .succeeded, result: down.result(stopped: 0)))?
                .message == "erp-development released its lease. Nothing needed to stop.")
        #expect(
            Self.feedback(down.downOperation(state: .cancelled, stage: "stop"))?.message
                == "Bringing down erp-development was cancelled. Its lease is released.")
        #expect(
            Self.feedback(down.downOperation(state: .cancelled, stage: nil))?.message
                == "Bringing down erp-development was cancelled before it started.")
        #expect(Self.feedback(down.downOperation()) == nil)
    }

    @Test func anIncompleteDownNamesWhatDidntStopWithShowOperation() throws {
        let down = FixtureEnvironmentDown.self
        let one = try #require(
            Self.feedback(
                down.downOperation(
                    state: .failed,
                    failure: down.incompleteFailure(stopped: 1, total: 2, instances: ["cache"]))))
        #expect(one.message == "erp-development is not fully down. cache didn’t stop.")
        #expect(one.tone == .caution)
        #expect(one.action?.title == "Show Operation")
        let two = try #require(
            Self.feedback(
                down.downOperation(
                    state: .failed,
                    failure: down.incompleteFailure(
                        stopped: 0, total: 2, instances: ["cache", "queue"]))))
        #expect(two.message == "erp-development is not fully down. 2 instances didn’t stop.")
    }

    @Test func aDownThatLeftInstancesRunningOffersBringDownAgain() throws {
        let down = FixtureEnvironmentDown.self
        let feedback = try #require(
            Self.feedback(
                down.downOperation(
                    state: .failed,
                    failure: down.incompleteFailure(
                        stopped: 0, total: 1, instances: ["cache"], outcome: "left_running"))))
        #expect(
            feedback.message
                == "erp-development released its lease, but cache is still running. Bring it down again."
        )
        #expect(feedback.action?.title == "Bring Down…")
        #expect(feedback.action?.mutates == true)
    }

    @Test func longListsEndWithHowManyMore() {
        let names = (1...8).map { "i\($0)" }
        #expect(EnvironmentCopy.names(names) == "i1, i2, i3, i4, i5, i6 and 2 more")
        #expect(EnvironmentCopy.names(["a", "b"]) == "a, b")
    }
}

/// The toolbar, banner, and Activity of a Down (spec 0047, AC-20, AC-21).
@MainActor
struct EnvironmentDownPageTests {
    @Test func downIsOffWhileADownRunsAndOnWhileAnUpRuns() {
        let running = EnvironmentDetailPage.downAction(
            summary: FixtureEnvironmentDown.bringingDownSummary, isBusy: false, perform: {})
        #expect(running.disabledReason == "Already bringing down")
        #expect(running.mutates)
        let up = EnvironmentDetailPage.downAction(
            summary: FixtureEnvironmentUp.bringingUpSummary, isBusy: false, perform: {})
        #expect(up.disabledReason == nil)
        #expect(up.title == "Down")
    }

    @Test func upIsOffWithBringingDownWhileADownRuns() {
        #expect(
            EnvironmentDetailPage.upDisabledReason(
                summary: FixtureEnvironmentDown.bringingDownSummary, manifestState: .inSync,
                problemCount: 0, declaredName: nil) == "Bringing down")
    }

    @Test func theBannerCountsTheStops() {
        #expect(EnvironmentCopy.bringingDown(done: 1, total: 2) == "Bringing down… 1 of 2")
        #expect(EnvironmentCopy.bringingDown(done: nil, total: nil) == "Bringing down…")
        #expect(
            StatusAppearance.environment(FixtureEnvironmentDown.bringingDownSummary).label
                == "Bringing down…")
    }

    @Test func activityTitlesTheDownAndMarksAnIncompleteOnePartial() {
        let down = FixtureEnvironmentDown.self
        let operation = down.downOperation(
            state: .failed,
            failure: down.incompleteFailure(stopped: 1, total: 2, instances: ["cache"]))
        #expect(
            OperationTitle.title(
                for: operation, instanceNames: [:],
                environments: [FixtureEnvironments.erpID: "erp-development"])
                == "Bring down environment erp-development")
        #expect(
            OperationTitle.title(for: operation, instanceNames: [:]) == "Bring down environment")
        #expect(StatusAdapter.appearance(of: operation).label == "Partial")
        #expect(OperationCancel.appearance(of: operation).label == "Partial")
        #expect(OperationNote.line(for: operation).text == "1 of 2 instances stopped")
        let none = down.downOperation(
            state: .failed,
            failure: down.incompleteFailure(stopped: 0, total: 1, instances: ["cache"]))
        #expect(StatusAdapter.downIncomplete(none) == nil)
        #expect(StatusAdapter.appearance(of: none).label != "Partial")
    }
}
