import Foundation

/// One of the five steps of the onboarding, in the order they show (spec 0016, AC-2).
nonisolated public enum OnboardingStep: Int, CaseIterable, Sendable {
    /// What Stackly is.
    case welcome
    /// The background agent and Login Items.
    case backgroundAgent
    /// Where the data lives.
    case storage
    /// Where the catalog comes from.
    case catalogTrust
    /// The `stackly` command in `~/.local/bin`.
    case commandLine

    /// The step's label, as the footer and VoiceOver say it.
    public var label: String {
        OnboardingCopy.stepLabels[rawValue]
    }

    /// The step's position, counting from 1.
    public var number: Int {
        rawValue + 1
    }

    /// The step after this one, or `nil` for the last.
    var following: OnboardingStep? {
        Self(rawValue: rawValue + 1)
    }

    /// The step before this one, or `nil` for the first.
    var preceding: OnboardingStep? {
        Self(rawValue: rawValue - 1)
    }
}
