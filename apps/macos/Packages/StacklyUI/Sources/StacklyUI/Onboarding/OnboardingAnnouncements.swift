import Foundation

/// The one announcement the onboarding makes about the background agent (spec 0016,
/// *Kartu Background Agent*, AC-10). Pure, so each presence and each pair of states is
/// tested as a value.
nonisolated enum OnboardingAnnouncements {
    /// The title to announce when the agent state turned to `not_found` while the window
    /// banner is hidden, or `nil` to stay quiet.
    ///
    /// `DaemonModel` announces the same state when the banner shows no agent row, so exactly
    /// one of the two speaks: this one when `AgentBanner.content` has a row for `input`,
    /// that one when it has not. The words are the banner row's title, the same ones
    /// `StateBanner` would say.
    ///
    /// - Parameters:
    ///   - old: The state before the change, or `nil` for the first one after launch, which
    ///     is never announced.
    ///   - new: The state after the change.
    ///   - input: `DaemonModel.agentBannerInput`, or `nil` without an agent.
    ///   - now: The time now; the `not_found` row does not depend on it.
    static func agentNotFound(
        old: BackgroundAgentState?, new: BackgroundAgentState?, input: AgentBanner.Input?,
        now: Date
    ) -> String? {
        guard let old, old != .notFound, new == .notFound, let input else { return nil }
        return AgentBanner.content(input, now: now)?.title
    }
}
