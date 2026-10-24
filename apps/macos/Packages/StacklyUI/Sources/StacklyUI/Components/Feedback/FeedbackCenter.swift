import Foundation
import Observation
import SwiftUI

/// The root view of the window creates it and puts it in the `feedbackCenter` environment
/// value; ``SwiftUI/View/feedbackPresenter()`` shows the card. A view inside a sheet never
/// sends to it: the sheet shows its own result with a banner, and the screen that owns the
/// sheet sends feedback after it closes.
@Observable
public final class FeedbackCenter {
    /// The feedback on screen now, or `nil`.
    public private(set) var current: Feedback?

    @ObservationIgnored private let sleep: @Sendable (Duration) async throws -> Void

    /// Creates a center.
    /// - Parameters:
    ///   - sleep: Waits for a duration and throws when cancelled; `Task.sleep(for:)` by     default.
    public init(

        sleep: @escaping @Sendable (Duration) async throws -> Void = {
            try await Task.sleep(for: $0)
        }
    ) {
        self.sleep = sleep
    }

    /// Anything else replaces what is on screen.
    public func show(_ feedback: Feedback) {
        if let current, current.isPersistent, feedback.isPersistent == false {
            return
        }
        current = feedback
    }

    /// Takes the feedback off the screen.
    public func dismiss() {
        current = nil
    }

    /// Waits ``Feedback/visibleDuration`` and then clears the feedback with `id`, if it is
    /// still on screen and not persistent.
    /// - Parameter id: The ID of the feedback that was shown.
    public func countdown(_ id: Feedback.ID) async {
        guard let feedback = current, feedback.id == id, feedback.isPersistent == false else {
            return
        }
        do {
            try await sleep(feedback.visibleDuration)
        } catch {
            return
        }
        if current?.id == id {
            current = nil
        }
    }
}

extension EnvironmentValues {
    /// The feedback center of this window, or `nil` where the root view sets none (a
    /// preview): then nothing is drawn.
    @Entry public var feedbackCenter: FeedbackCenter? = nil
}
