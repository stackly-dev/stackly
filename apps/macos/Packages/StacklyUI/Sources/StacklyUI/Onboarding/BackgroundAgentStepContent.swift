import Foundation

/// What the card of the Background Agent step shows (spec 0016, *Kartu Background Agent*):
/// one row of the table, chosen from the first line that fits. Pure, so every row is tested
/// and snapshotted without a model.
nonisolated struct BackgroundAgentStepContent: Equatable {
    /// The icon column.
    enum Icon: Equatable {
        /// A progress indicator that shows after a short delay.
        case progress
        /// A symbol in the fill of `tone`.
        case symbol(String, Tone)
    }

    /// The one action on the right of the card.
    enum Action: Equatable {
        /// `BackgroundAgentModel.turnOn()`.
        case turnOn
        /// `BackgroundAgentModel.openLoginItems()`.
        case openLoginItems
        /// `BackgroundAgentModel.start()`.
        case start
        /// `BackgroundAgentModel.refresh()`.
        case checkAgain

        /// The button title.
        var title: String {
            switch self {
            case .turnOn: OnboardingCopy.actionTurnOn
            case .openLoginItems: OnboardingCopy.actionOpenSettings
            case .start: OnboardingCopy.actionStart
            case .checkAgain: OnboardingCopy.actionCheckAgain
            }
        }
    }

    /// The icon.
    let icon: Icon
    /// The row title.
    let title: String
    /// The row detail, or `nil` for the row that only checks.
    let detail: String?
    /// The daemon status under the detail, on every enabled row.
    let status: StatusAppearance?
    /// The action, or `nil` for a row without one.
    let action: Action?
    /// Whether an action of the model runs, which disables the action and shows progress.
    let isWorking: Bool
    /// The text of the last failed action, shown under the card, or `nil`.
    let lastError: String?

    /// Builds the content.
    /// - Parameters:
    ///   - input: The input of the window banner (`DaemonModel.agentBannerInput`), so the
    ///     banner, the Diagnostics hint, and the card never differ.
    ///   - footer: The daemon status of the sidebar footer (`ShellPresentation.footer`).
    ///   - now: The time now (`fixtureNow` where there is one).
    ///   - lastError: What the last action of the model failed with.
    ///   - isWorking: Whether an action or the wait for the daemon runs.
    static func make(
        _ input: AgentBanner.Input, footer: StatusAppearance, now: Date,
        lastError: AgentActionError?, isWorking: Bool
    ) -> BackgroundAgentStepContent {
        let row = row(for: input, footer: footer, now: now)
        let showsError = row.hidesLastError == false
        return BackgroundAgentStepContent(
            icon: row.icon, title: row.title, detail: row.detail, status: row.status,
            action: row.action, isWorking: isWorking,
            lastError: showsError ? lastError?.message : nil)
    }

    /// Whether `a` and `b` are the same row of the table, whatever `isWorking` and the last
    /// error show.
    static func isSameRow(_ a: BackgroundAgentStepContent, _ b: BackgroundAgentStepContent)
        -> Bool
    {
        a.title == b.title && a.detail == b.detail && a.action == b.action
    }

    /// The 15 seconds after which a missing daemon is offered Start stacklyd.
    private static let graceSeconds = TimeInterval(AgentTiming.startGrace.components.seconds)

    // MARK: - The table

    private struct Row {
        var icon: Icon
        var title: String
        var detail: String?
        var status: StatusAppearance?
        var action: Action?
        /// An enabled row of a connected daemon: its Last error stays in Diagnostics.
        var hidesLastError = false
    }

    /// How the daemon presence reads for an enabled agent.
    private enum Reach {
        case connected
        case connecting
        case missing(since: Date)
        case other
    }

    private static func reach(of input: AgentBanner.Input) -> Reach {
        // The card does not open `.refreshing(after:)`: the daemon answered the handshake.
        switch input.status {
        case .connected, .refreshing, .starting:
            return .connected
        case .connecting:
            return .connecting
        case .unreachable(_, let since):
            if let code = input.failureCode, AgentBanner.failureCodes.contains(code) {
                return .missing(since: since)
            }
            return .other
        case .stopping, .incompatible, .untrusted, .recovery, .misconfigured:
            return .other
        }
    }

    private static func row(
        for input: AgentBanner.Input, footer: StatusAppearance, now: Date
    ) -> Row {
        guard let assessment = input.assessment else {
            return Row(icon: .progress, title: OnboardingCopy.agentCheckingTitle)
        }
        switch assessment.state {
        case .enabled:
            return enabledRow(input, footer: footer, now: now)
        case .notRegistered:
            return Row(
                icon: .symbol(Tone.caution.symbol, .caution), title: OnboardingCopy.agentOffTitle,
                detail: OnboardingCopy.agentOffDetail, action: .turnOn)
        case .requiresApproval:
            return Row(
                icon: .symbol("clock", .caution), title: OnboardingCopy.agentApprovalTitle,
                detail: OnboardingCopy.agentApprovalDetail, action: .openLoginItems)
        case .disabled:
            return Row(
                icon: .symbol(Tone.caution.symbol, .caution),
                title: OnboardingCopy.agentDisabledTitle,
                detail: OnboardingCopy.agentDisabledDetail, action: .openLoginItems)
        case .notFound:
            return Row(
                icon: .symbol(Tone.critical.symbol, .critical),
                title: OnboardingCopy.agentNotFoundTitle,
                detail: OnboardingCopy.agentNotFoundDetail)
        case .unavailable:
            return unavailableRow(assessment.reason, detail: input.detail)
        }
    }

    private static func enabledRow(
        _ input: AgentBanner.Input, footer: StatusAppearance, now: Date
    ) -> Row {
        let title = OnboardingCopy.agentAllowedTitle
        switch reach(of: input) {
        case .connected:
            return Row(
                icon: .symbol(Tone.positive.symbol, .positive), title: title,
                detail: OnboardingCopy.agentConnectedDetail, status: footer,
                hidesLastError: true)
        case .connecting:
            return Row(
                icon: .symbol(Tone.informative.symbol, .informative), title: title,
                detail: OnboardingCopy.agentConnectingDetail, status: footer)
        case .missing(let since):
            if input.isStarting || now.timeIntervalSince(since) < graceSeconds {
                return Row(
                    icon: .symbol(Tone.informative.symbol, .informative), title: title,
                    detail: OnboardingCopy.agentStartingDetail, status: footer)
            }
            return Row(
                icon: .symbol(Tone.caution.symbol, .caution),
                title: OnboardingCopy.agentNotRunningTitle,
                detail: OnboardingCopy.agentNotRunningDetail, status: footer, action: .start)
        case .other:
            return Row(
                icon: .symbol(footer.tone.symbol, footer.tone), title: title,
                detail: OnboardingCopy.agentCannotReachDetail, status: footer)
        }
    }

    private static func unavailableRow(_ reason: UnavailableReason?, detail: String?) -> Row {
        switch reason {
        case .helperMissing, .notBundled:
            Row(
                icon: .symbol(Tone.caution.symbol, .caution),
                title: OnboardingCopy.agentNoHelperTitle,
                detail: OnboardingCopy.agentNoHelperDetail)
        case .rootOverride:
            Row(
                icon: .symbol(Tone.caution.symbol, .caution),
                title: OnboardingCopy.agentRootOverrideTitle,
                detail: OnboardingCopy.agentRootOverrideDetail)
        case .statusFailed, .statusUnrecognized, .unsupportedOs, nil:
            Row(
                icon: .symbol(Tone.caution.symbol, .caution),
                title: OnboardingCopy.agentUnavailableTitle,
                detail: OnboardingCopy.agentUnavailableDetail(detail), action: .checkAgain)
        }
    }
}
