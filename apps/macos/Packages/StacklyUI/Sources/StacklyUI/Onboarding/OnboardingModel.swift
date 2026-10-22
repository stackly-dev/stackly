import Foundation
import Observation

/// How the onboarding was closed.
public enum OnboardingFinish: Sendable, Equatable {
    /// The Get Started button on the last step.
    case getStarted
    /// Escape, or the VoiceOver escape action, on any step.
    case skipped
}

/// Whether the onboarding shows in the main window, and at which step (spec 0016, *State
/// onboarding*).
///
/// The runtime owns it, so closing and opening the window again shows the same step. Moving
/// between steps changes nothing else; the one thing written is the record, and only by
/// ``finish(_:)``.
@Observable
public final class OnboardingModel {
    /// Whether the onboarding replaces the shell.
    public private(set) var isPresented: Bool
    /// The step on screen.
    public private(set) var step: OnboardingStep = .welcome

    @ObservationIgnored private let store: any OnboardingRecordStore

    /// Creates the model. The onboarding shows unless the store says it is finished.
    ///
    /// - Parameter store: Where the finished record lives.
    public init(store: any OnboardingRecordStore) {
        self.store = store
        isPresented = store.isFinished == false
    }

    /// Shows the onboarding at the first step, for the Help menu. While it already shows,
    /// the step stays.
    public func present() {
        guard isPresented == false else { return }
        step = .welcome
        isPresented = true
    }

    /// Moves to the next step. Nothing happens on the last step.
    public func next() {
        if let following = step.following { step = following }
    }

    /// Moves to the previous step. Nothing happens on the first step.
    public func back() {
        if let preceding = step.preceding { step = preceding }
    }

    /// Moves straight to `target`.
    public func go(to target: OnboardingStep) {
        step = target
    }

    /// Closes the onboarding and records that it is finished. Nothing happens while it is
    /// hidden.
    ///
    /// - Parameter reason: How it was closed. Both reasons write the same record.
    public func finish(_ reason: OnboardingFinish) {
        guard isPresented else { return }
        store.markFinished()
        isPresented = false
    }
}
