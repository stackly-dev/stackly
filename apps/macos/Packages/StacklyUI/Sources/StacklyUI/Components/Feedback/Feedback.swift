import Foundation
import StacklyClient

/// The result of an action shown to the person: a tone, a message, and optionally a
/// detail, a code, and one action (spec 0067, AC-9 and AC-10). A ``FeedbackCenter`` shows
/// it as a card under the page.
///
/// Use it only for the result of something the person did, never for a change that happens
/// on its own. A failure is always also visible where it happened (Needs Attention, the
/// item's banner, Operations), so the card is never the only record.
public struct Feedback: Identifiable {
    /// Tells one feedback from another: a new feedback is a new card.
    public let id: UUID
    /// The tone: its symbol and tint.
    public var tone: Tone
    /// The message, in one sentence.
    public var message: String
    /// More words under the message, or `nil`.
    public var detail: String?
    /// A code the person can select and copy, or `nil`.
    public var code: String?
    /// The one button of the card, or `nil`.
    public var action: StacklyAction?

    /// Creates a feedback.
    ///
    /// - Parameters:
    ///   - tone: The tone.
    ///   - message: The message.
    ///   - detail: More words, or `nil`.
    ///   - code: A code to copy, or `nil`.
    ///   - action: The one button, or `nil`.
    public init(
        tone: Tone,
        message: String,
        detail: String? = nil,
        code: String? = nil,
        action: StacklyAction? = nil
    ) {
        id = UUID()
        self.tone = tone
        self.message = message
        self.detail = detail
        self.code = code
        self.action = action
    }

    /// Whether the card stays until the person dismisses it or something replaces it: a
    /// `caution` or `critical` tone, or any feedback with an action. Everything else goes
    /// away after ``visibleDuration``.
    public var isPersistent: Bool {
        tone == .caution || tone == .critical || action != nil
    }

    /// How long a card that is not persistent stays: `min(10, max(4, ceil(characters / 15)))`
    /// seconds, counting the message and the detail, roughly the reading speed of 15
    /// characters a second with a floor of 4 seconds.
    public var visibleDuration: Duration {
        let characters = message.count + (detail?.count ?? 0)
        let seconds = min(10, max(4, (characters + 14) / 15))
        return .seconds(seconds)
    }

    /// Good news of the person's action: `positive`, with no action.
    public static func success(_ message: String, detail: String? = nil) -> Feedback {
        Feedback(tone: .positive, message: message, detail: detail)
    }

    /// An operation that started: `informative`, with no action.
    public static func started(_ message: String, detail: String? = nil) -> Feedback {
        Feedback(tone: .informative, message: message, detail: detail)
    }

    /// A plain note, such as a cancelled operation: `neutral`, with no action.
    public static func neutral(_ message: String, detail: String? = nil) -> Feedback {
        Feedback(tone: .neutral, message: message, detail: detail)
    }

    /// A failure from the daemon: `critical`, with the daemon's message and hint as
    /// sentences and its error code.
    ///
    /// - Parameter body: The error of a rejected call or of a failed operation
    ///   (`OperationFailureInfo.error`).
    public static func failure(_ body: ErrorBody) -> Feedback {
        Feedback(
            tone: .critical,
            message: DaemonText.sentence(body.message),
            detail: body.hint.map(DaemonText.sentence),
            code: body.code
        )
    }

    /// The feedback of a mutation's outcome (spec 0067, *Pemetaan hasil mutasi*).
    ///
    /// - Parameters:
    ///   - outcome: How the mutation ended.
    ///   - success: The feedback to show when it completed, or `nil` for none.
    ///   - showOperations: Opens Operations, or `nil` when the screen has no way to it. It
    ///     becomes the "Show Operations" button of an unknown outcome.
    /// - Returns: The feedback, or `nil` when a completed mutation has none to show.
    public static func outcome(
        _ outcome: MutationOutcome,
        success: Feedback?,
        showOperations: (@MainActor () -> Void)?
    ) -> Feedback? {
        switch outcome {
        case .completed:
            return success
        case .rejected(let body):
            return failure(body)
        case .notSent:
            return Feedback(tone: .caution, message: MutationFeedback.notSent)
        case .outcomeUnknown:
            return Feedback(
                tone: .caution,
                message: MutationFeedback.outcomeUnknown,
                action: showOperations.map {
                    StacklyAction(
                        id: "feedback.showOperations",
                        title: "Show Operations",
                        mutates: false,
                        perform: $0
                    )
                }
            )
        }
    }
}
