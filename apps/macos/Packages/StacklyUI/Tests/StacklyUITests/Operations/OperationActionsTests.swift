import Foundation
import StacklyClient
import StacklyTestSupport
import Testing

@testable import StacklyUI

/// Cancel, Resume, and Abandon of the Operations page and its inspector detail (spec 0035
/// AC-9, AC-10, AC-12): each sends one call with a new key and the revision the page shows,
/// puts the reply in the list, and reports by disposition; a stale revision reads the
/// operation again; an operation the daemon no longer knows shows as gone.
@MainActor
struct OperationActionsTests {
    private func rig(_ operations: [OperationSummary]) async -> (
        OperationsFake, OperationHistory, OperationActions
    ) {
        let fake = OperationsFake()
        fake.pages[nil] = OperationPage(items: operations, next: nil)
        let history = OperationHistory(service: fake.service, center: OperationsFake.center())
        await history.reload()
        return (fake, history, OperationActions(service: fake.service, history: history))
    }

    @Test(
        .evidence(
            covers: ["REQ-054", "AC-054"],
            expected:
                "a cancel past the point of no return shows that the operation finishes on its own and the row turns to Finishing safely, never to cancelled"
        ))
    func aCancelPastThePointOfNoReturnSaysItFinishesOnItsOwn() async {
        let running = OperationSample.make(
            kind: .installRuntime, instance: nil, state: .running, stage: "commit",
            cancel: .available, revision: 4)
        let (fake, history, actions) = await rig([running])
        let continuing = OperationSample.make(
            kind: .installRuntime, instance: nil, state: .cancelRequested, stage: "commit",
            cancel: .continuing, cancelRequestedAt: OperationSample.base, revision: 5)
        fake.outcomes = [
            .completed(
                .operationCancel(
                    CancelReply(operation: continuing, disposition: .continuingPastPointOfNoReturn))
            )
        ]
        var cards: [Feedback] = []

        await actions.cancel(running, title: "Install PostgreSQL 17.6") { cards.append($0) }

        guard case .operationCancel(let params) = fake.sent.first?.call else {
            Issue.record("expected operation.cancel")
            return
        }
        #expect(params.operationId == running.id)
        #expect(fake.sent.first?.key.isEmpty == false)
        #expect(
            cards.map(\.message) == [
                "It’s past a step that can’t be undone, so it will finish on its own."
            ])
        #expect(history.items.first?.revision == 5)
        #expect(OperationNote.line(for: history.items[0]).text == "Finishing safely · Commit")
        #expect(OperationCancel.appearance(of: history.items[0]).label == "Finishing…")
        #expect(actions.inFlight.isEmpty)
    }

    @Test func eachCancelDispositionHasItsCard() {
        let title = "Start db"
        #expect(
            OperationActions.feedback(.cancelledBeforeStart, title: title)?.message
                == "Cancelled “Start db”. It hadn’t started, so nothing of it ran.")
        #expect(
            OperationActions.feedback(.requested, title: title)?.message
                == "Cancelling “Start db”. It stops at its next safe point.")
        #expect(
            OperationActions.feedback(.alreadyFinal, title: title)?.message
                == "“Start db” had already ended.")
        #expect(
            OperationActions.feedback(.recoveryRequired, title: title)?.tone == .caution)
        #expect(OperationActions.feedback(.unrecognized, title: title) == nil)
    }

    @Test(
        .evidence(
            covers: ["REQ-054"],
            expected:
                "Resume and Abandon send the revision the page shows, each with a new idempotency key, and Abandon reports the items left"
        ))
    func resumeAndAbandonSendTheShownRevisionWithNewKeys() async {
        let stuck = OperationSample.make(
            state: .needsRecovery,
            recovery: OperationSample.recovery(.resume, actions: [.resume, .abandon]),
            revision: 7)
        let (fake, history, actions) = await rig([stuck])
        let resumed = OperationSample.make(state: .queued, revision: 8)
        let abandoned = OperationSample.make(state: .failed, revision: 9)
        let leftover = CreatedItemInfo(
            kind: "instance_tree", reference: "local:x", state: .kept, detail: nil)
        fake.outcomes = [
            .completed(.operationResume(resumed)),
            .completed(
                .operationAbandon(
                    AbandonReply(operation: abandoned, leftovers: [leftover, leftover]))),
        ]
        var cards: [Feedback] = []

        await actions.resume(stuck, title: "Start db") { cards.append($0) }
        await actions.abandon(stuck, title: "Start db") { cards.append($0) }

        #expect(
            fake.sent.map(\.call) == [
                .operationResume(ResumeOperationParams(operationId: stuck.id, expectedRevision: 7)),
                .operationAbandon(
                    AbandonOperationParams(operationId: stuck.id, expectedRevision: 7)),
            ])
        #expect(Set(fake.sent.map(\.key)).count == 2)
        #expect(cards.map(\.message) == ["Resuming “Start db”.", "Abandoned “Start db”."])
        #expect(cards.last?.detail == "2 items were left as they are.")
        #expect(history.items.first?.revision == 9)
    }

    @Test(
        .evidence(
            covers: ["REQ-054"],
            expected:
                "an abandon refused with resource.revision_mismatch reads the operation again and shows the refusal"
        ))
    func aStaleRevisionReadsTheOperationAgainAndShowsTheRefusal() async {
        let stuck = OperationSample.make(
            state: .needsRecovery,
            recovery: OperationSample.recovery(.abandon, actions: [.abandon]), revision: 7)
        let (fake, history, actions) = await rig([stuck])
        let newer = OperationSample.make(
            state: .needsRecovery,
            recovery: OperationSample.recovery(.abandon, actions: [.abandon]), revision: 8)
        fake.details[stuck.id] = OperationsFake.detail(newer)
        let refusal = OperationsFake.error(
            "resource.revision_mismatch", "the operation changed since you looked")
        fake.outcomes = [.rejected(refusal)]
        var cards: [Feedback] = []

        await actions.abandon(stuck, title: "Start db") { cards.append($0) }

        #expect(history.items.first?.revision == 8)
        #expect(cards.count == 1)
        #expect(cards.first?.code == "resource.revision_mismatch")
    }

    @Test func aMutationThatNeverReachedTheDaemonSaysSo() async {
        let running = OperationSample.make(state: .running, cancel: .available)
        let (_, history, actions) = await rig([running])
        var cards: [Feedback] = []
        await actions.cancel(running, title: "Start db") { cards.append($0) }
        #expect(cards.map(\.message) == [MutationFeedback.notSent])
        #expect(history.items.first?.revision == running.revision)
    }

    @Test func theAbandonConfirmationIsDestructiveCopy() {
        let confirmation = OperationActions.abandonConfirmation(title: "Restore db")
        #expect(confirmation.title == "Abandon “Restore db”?")
        #expect(
            confirmation.message
                == "Stackly undoes what it can and leaves the rest as it is. You can’t resume it afterwards."
        )
        #expect(confirmation.actionTitle == "Abandon")
    }

    // MARK: - Detail

    @Test(
        .evidence(
            covers: ["REQ-054"],
            expected:
                "the inspector reads operation.get, keeps the last detail while a newer revision is read, and says the operation is gone when the daemon no longer knows it"
        ))
    func theDetailLoadsKeepsItsLastValueAndKnowsAGoneOperation() async {
        let fake = OperationsFake()
        let history = OperationHistory(service: fake.service, center: OperationsFake.center())
        let detail = OperationDetailModel(history: history)
        let operation = OperationSample.make(2, state: .failed, revision: 3)
        let step = OperationStepInfo(
            seq: 1, name: "endpoints", state: .failed, repeatable: true, reversible: true,
            attempts: 2, startedAt: OperationSample.base, finishedAt: nil,
            errorCode: "supervisor.endpoint_in_use")
        fake.details[operation.id] = OperationDetail(
            operation: operation, steps: [step], created: [])

        #expect(detail.phase(for: operation.id) == .loading)
        await detail.load(operation.id, revision: 3)
        guard case .loaded(let loaded) = detail.phase(for: operation.id) else {
            Issue.record("expected the detail")
            return
        }
        #expect(loaded.steps?.first?.errorCode == "supervisor.endpoint_in_use")
        // It also joined the list.
        #expect(history.items.map(\.id) == [operation.id])

        // A failed read of the same operation keeps the detail on screen.
        fake.details[operation.id] = nil
        fake.details[OperationSample.id(9)] = nil
        let gone = OperationSample.id(9)
        await detail.load(gone, revision: nil)
        #expect(detail.phase(for: gone) == .missing)
        #expect(history.missing.contains(gone))
        #expect(detail.phase(for: operation.id) == .loading)
    }

    @Test func theStepAndMetaWordsReadWell() {
        let utc = TimeZone(identifier: "UTC") ?? .gmt
        let locale = Locale(identifier: "en_US")
        let step = OperationStepInfo(
            seq: 1, name: "endpoints", state: .failed, repeatable: true, reversible: true,
            attempts: 2, startedAt: OperationSample.base, finishedAt: nil,
            errorCode: "supervisor.endpoint_in_use")
        #expect(
            OperationsCopy.stepLine(step, locale: locale, timeZone: utc)
                == "Failed · 2 attempts · 7:33:20\u{202F}AM · supervisor.endpoint_in_use")
        let operation = OperationSample.make(2)
        #expect(
            OperationsCopy.meta(operation, locale: locale, timeZone: utc)
                == "7:33\u{202F}AM · 0192f3a4")
        #expect(OperationsCopy.kind(.changeEndpoints) == "Change endpoints")
        #expect(OperationsCopy.by(OperationTriggerInfo(origin: .user, client: .cli)) == "CLI")
        #expect(OperationsCopy.by(OperationTriggerInfo(origin: .user, client: .gui)) == "App")
        #expect(
            OperationsCopy.by(OperationTriggerInfo(origin: .background, client: nil)) == "Stackly")
        #expect(OperationsCopy.by(OperationTriggerInfo(origin: .user, client: nil)) == "Client")
    }
}
