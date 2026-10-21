import Foundation
import StacklyClient
import StacklyTestSupport
import Testing

@testable import StacklyUI

/// The feedback values: outcome mapping, failure, persistence, and duration (spec 0067,
/// AC-9, AC-10).
struct FeedbackTests {
    private func body(
        code: String = "instance.port_in_use",
        message: String = "port 5432 is already in use",
        hint: String? = "choose another port"
    ) -> ErrorBody {
        ErrorBody(
            code: code, category: .conflict, message: message, retryable: false, resource: nil,
            fields: nil, hint: hint, details: nil, operationId: nil, rule: nil, items: nil)
    }

    private var unknownBody: ErrorBody { body(code: "ipc.outcome_unknown", message: "lost") }

    // MARK: - outcome

    @Test func aCompletedMutationGivesTheScreensSuccessOrNothing() throws {
        let success = Feedback.success("Instance started.")
        let shown = try #require(
            Feedback.outcome(.completed(.eventsUnsubscribe), success: success, showOperations: nil))
        #expect(shown.id == success.id)
        #expect(shown.tone == .positive)
        #expect(
            Feedback.outcome(.completed(.eventsUnsubscribe), success: nil, showOperations: nil)
                == nil)
    }

    @Test(
        .evidence(
            covers: ["REQ-024"],
            expected:
                "Spec 0067 AC-10: a rejected mutation is critical with the daemon's sentences and code"
        ))
    func aRejectedMutationIsCriticalWithTheDaemonsWordsAsSentencesAndItsCode() throws {
        let feedback = try #require(
            Feedback.outcome(.rejected(body()), success: .success("ignored"), showOperations: nil))
        #expect(feedback.tone == .critical)
        #expect(feedback.message == "Port 5432 is already in use.")
        #expect(feedback.detail == "Choose another port.")
        #expect(feedback.code == "instance.port_in_use")
        #expect(feedback.action == nil)
        #expect(feedback.isPersistent)
    }

    @Test func aRejectedMutationWithoutAHintHasNoDetail() throws {
        let feedback = try #require(
            Feedback.outcome(.rejected(body(hint: nil)), success: nil, showOperations: nil))
        #expect(feedback.detail == nil)
    }

    @Test func aMessageThatAlreadyEndsWithAPeriodKeepsOne() {
        let feedback = Feedback.failure(body(message: "Already done.", hint: "retry later."))
        #expect(feedback.message == "Already done.")
        #expect(feedback.detail == "Retry later.")
    }

    @Test(
        .evidence(
            covers: ["REQ-024"],
            expected:
                "Spec 0067 AC-10: a mutation that was not sent is a caution with the shell's words")
    )
    func aMutationThatWasNotSentIsACautionWithTheShellsWords() throws {
        let outcome = Feedback.outcome(
            .notSent(body()), success: .success("ignored"), showOperations: {})
        let feedback = try #require(outcome)
        #expect(feedback.tone == .caution)
        #expect(feedback.message == "stacklyd is unavailable. Nothing was sent.")
        #expect(feedback.message == MutationFeedback.notSent)
        #expect(feedback.detail == nil)
        #expect(feedback.code == nil)
        #expect(feedback.action == nil)
    }

    @Test(
        .evidence(
            covers: ["REQ-024"],
            expected:
                "Spec 0067 AC-10: an unknown outcome offers Show Operations only when the screen can open them"
        ))
    func anUnknownOutcomeOffersShowOperationsOnlyWhenTheScreenCanOpenThem() throws {
        var opened = 0
        let outcome = Feedback.outcome(
            .outcomeUnknown(unknownBody, key: "k"), success: nil,
            showOperations: { opened += 1 })
        let with = try #require(outcome)
        #expect(with.tone == .caution)
        #expect(with.message == MutationFeedback.outcomeUnknown)
        let action = try #require(with.action)
        #expect(action.title == "Show Operations")
        #expect(action.mutates == false)
        #expect(action.availability(gate: .blocked(reason: "x", since: .now)) == .enabled)
        action.perform()
        #expect(opened == 1)

        let without = try #require(
            Feedback.outcome(
                .outcomeUnknown(unknownBody, key: "k"), success: nil, showOperations: nil))
        #expect(without.action == nil)
    }

    @Test(
        .evidence(
            covers: ["REQ-024"],
            expected:
                "Spec 0067 AC-10: a failed operation gives the same critical feedback as a rejection"
        ))
    func aFailedOperationGivesTheSameCriticalFeedbackAsARejection() {
        let feedback = Feedback.failure(body())
        #expect(feedback.tone == .critical)
        #expect(feedback.message == "Port 5432 is already in use.")
        #expect(feedback.code == "instance.port_in_use")
    }

    // MARK: - constructors

    @Test func theConvenienceConstructorsSetTheToneAndNoAction() {
        #expect(Feedback.success("a").tone == .positive)
        #expect(Feedback.started("a", detail: "b").tone == .informative)
        #expect(Feedback.started("a", detail: "b").detail == "b")
        #expect(Feedback.neutral("a").tone == .neutral)
        for feedback in [Feedback.success("a"), .started("a"), .neutral("a")] {
            #expect(feedback.action == nil)
        }
    }

    // MARK: - persistence and duration

    @Test(
        .evidence(
            covers: ["REQ-024"],
            expected: "Spec 0067 AC-9: only caution, critical, or actionable feedback is persistent"
        ))
    func onlyCautionCriticalOrActionableFeedbackIsPersistent() {
        for tone in Tone.allCases {
            let expected = tone == .caution || tone == .critical
            #expect(Feedback(tone: tone, message: "m").isPersistent == expected, "\(tone)")
        }
        let action = StacklyAction(id: "a", title: "Open", mutates: false, perform: {})
        for tone in Tone.allCases {
            #expect(Feedback(tone: tone, message: "m", action: action).isPersistent, "\(tone)")
        }
    }

    @Test(
        .evidence(
            covers: ["REQ-024"],
            expected:
                "Spec 0067 AC-9: the visible duration follows the text length within four and ten seconds"
        ))
    func visibleDurationFollowsTheReadingSpeedWithinFourAndTenSeconds() {
        func seconds(_ message: String, detail: String? = nil) -> Duration {
            Feedback(tone: .positive, message: message, detail: detail).visibleDuration
        }
        #expect(seconds("Done.") == .seconds(4))
        #expect(seconds(String(repeating: "x", count: 60)) == .seconds(4))
        #expect(seconds(String(repeating: "x", count: 61)) == .seconds(5))
        #expect(seconds(String(repeating: "x", count: 100)) == .seconds(7))
        #expect(
            seconds(String(repeating: "x", count: 100), detail: String(repeating: "y", count: 50))
                == .seconds(10))
        #expect(seconds(String(repeating: "x", count: 300)) == .seconds(10))
    }

    // MARK: - DaemonText

    @Test func aSentenceHasACapitalAndAFinalPeriod() {
        #expect(DaemonText.sentence("port in use") == "Port in use.")
        #expect(DaemonText.sentence("Already a sentence.") == "Already a sentence.")
        #expect(DaemonText.sentence("") == "")
        #expect(DaemonText.sentence("ünïcode first") == "Ünïcode first.")
    }
}
