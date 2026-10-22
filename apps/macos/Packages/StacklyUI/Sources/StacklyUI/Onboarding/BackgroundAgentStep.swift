import SwiftUI

/// The Background Agent step: its header and the card of the agent's real state (spec 0016,
/// AC-5). It shows nothing of the card without a `DaemonModel` that has an agent. It reads
/// the state and turns the card's buttons into calls of `BackgroundAgentModel` and nothing
/// else; the step appearing only reads the status again.
struct BackgroundAgentStep: View {
    let focus: AccessibilityFocusState<Bool>.Binding
    /// Where keyboard focus sits in the onboarding.
    let keyFocus: FocusState<OnboardingFocusTarget?>.Binding

    @Environment(DaemonModel.self) private var model: DaemonModel?
    @Environment(\.fixtureNow) private var fixtureNow
    @State private var focusAfterCardAction = false

    var body: some View {
        VStack(spacing: Spacing.s14) {
            OnboardingStepHeader(
                title: OnboardingCopy.agentTitle, subtitle: OnboardingCopy.agentSubtitle,
                focus: focus)
            card
        }
        .task {
            // The step appearing only reads the status; it never registers anything.
            await model?.agent?.refresh()
        }
        .onChange(of: keyFocus.wrappedValue) { _, new in
            if CardFocusRule.releasesFlag(whenFocusMovesTo: new) { focusAfterCardAction = false }
        }
    }

    @ViewBuilder private var card: some View {
        // The model is read here, outside the timeline closure.
        if let model, let agent = model.agent, let input = model.agentBannerInput {
            let footer = model.presentation.footer
            let lastError = agent.lastError
            let isWorking = agent.isWorking
            if DaemonBanner.ticksEverySecond(input, now: fixtureNow ?? Date.now) {
                TimelineView(.periodic(from: .now, by: 1)) { context in
                    cardView(
                        input, footer: footer, lastError: lastError, isWorking: isWorking,
                        agent: agent, date: context.date)
                }
            } else {
                TimelineView(.everyMinute) { context in
                    cardView(
                        input, footer: footer, lastError: lastError, isWorking: isWorking,
                        agent: agent, date: context.date)
                }
            }
        }
    }

    private func cardView(
        _ input: AgentBanner.Input, footer: StatusAppearance, lastError: AgentActionError?,
        isWorking: Bool, agent: BackgroundAgentModel, date: Date
    ) -> some View {
        let content = BackgroundAgentStepContent.make(
            input, footer: footer, now: fixtureNow ?? date, lastError: lastError,
            isWorking: isWorking)
        return BackgroundAgentCard(
            content: content, perform: { action in perform(action, on: agent) }, focus: keyFocus
        )
        .onChange(of: content) { old, new in
            let outcome = CardFocusRule.outcome(
                old: old, new: new, focused: keyFocus.wrappedValue,
                afterAction: focusAfterCardAction
            )
            if outcome.releasesFlag { focusAfterCardAction = false }
            if let target = outcome.target { keyFocus.wrappedValue = target }
        }
    }

    private func perform(
        _ action: BackgroundAgentStepContent.Action, on agent: BackgroundAgentModel
    ) {
        focusAfterCardAction = true
        Task { await agent.run(action) }
    }
}

extension BackgroundAgentModel {
    /// Runs what a card button asks for: only `turnOn()`, `openLoginItems()`, `start()`, and
    /// `refresh()`, never `register`, `unregister`, or `relaunch` on its own.
    func run(_ action: BackgroundAgentStepContent.Action) async {
        switch action {
        case .turnOn: await turnOn()
        case .start: await start()
        case .checkAgain: await refresh()
        case .openLoginItems: openLoginItems()
        }
    }
}
