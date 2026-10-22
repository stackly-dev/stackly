import SwiftUI

/// The one window banner for the daemon's state, above every page: a ``StateBanner`` on
/// the window surface with the banner's actions.
///
/// Its time follows the clock, the calendar, the locale, and the time zone of the
/// environment, and updates every minute. While the background agent's row for an enabled
/// agent is still under 15 seconds old, it updates every second, so the row changes at
/// the 15th second. Reconnect shows a small spinner while an attempt runs. No action is
/// gated, because none changes data.
///
/// With a ``DaemonModel`` in the environment, the banner shows the agent's row from
/// ``DaemonModel/agentBannerInput`` whenever there is one, and `content` otherwise.
public struct DaemonBanner: View {
    @Environment(DaemonModel.self) private var model: DaemonModel?
    @Environment(\.fixtureNow) private var fixtureNow
    @Environment(\.calendar) private var calendar
    @Environment(\.locale) private var locale
    @Environment(\.timeZone) private var timeZone

    private let content: DaemonBannerContent
    private let isReconnecting: Bool
    private let reconnect: @MainActor () -> Void
    private let openDiagnostics: @MainActor () -> Void

    /// Creates the banner.
    ///
    /// - Parameters:
    ///   - content: The banner from ``ShellPresentation/banner``.
    ///   - isReconnecting: Whether a connection attempt runs now.
    ///   - reconnect: Tries to connect now.
    ///   - openDiagnostics: Shows the Diagnostics page.
    public init(
        _ content: DaemonBannerContent,
        isReconnecting: Bool,
        reconnect: @escaping @MainActor () -> Void,
        openDiagnostics: @escaping @MainActor () -> Void
    ) {
        self.content = content
        self.isReconnecting = isReconnecting
        self.reconnect = reconnect
        self.openDiagnostics = openDiagnostics
    }

    /// Whether the banner for `input` still changes at the 15th second, so a view that shows
    /// it redraws every second: true only for an enabled agent that is not starting and has
    /// been missing for less than ``AgentTiming/startGrace``.
    ///
    /// - Parameters:
    ///   - input: What the banner is built from, or `nil` without an agent.
    ///   - now: The time now (`fixtureNow` when there is one).
    public static func ticksEverySecond(_ input: AgentBanner.Input?, now: Date) -> Bool {
        AgentBanner.isCountingToGrace(input, now: now)
    }

    /// The banner's content.
    public var body: some View {
        // The model is read here, outside the timeline closure.
        let input = model?.agentBannerInput
        let agent = model?.agent
        let ticks = Self.ticksEverySecond(input, now: fixtureNow ?? Date.now)
        if ticks {
            TimelineView(.periodic(from: .now, by: 1)) { context in
                banner(input: input, agent: agent, date: context.date)
            }
        } else {
            TimelineView(.everyMinute) { context in
                banner(input: input, agent: agent, date: context.date)
            }
        }
    }

    private func banner(
        input: AgentBanner.Input?, agent: BackgroundAgentModel?, date: Date
    ) -> some View {
        let now = fixtureNow ?? date
        let shown = input.flatMap { AgentBanner.content($0, now: now) } ?? content
        return StateBanner(
            tone: shown.tone,
            title: shown.title,
            message: shown.message(
                now: now,
                calendar: calendar,
                locale: locale,
                timeZone: timeZone
            ),
            actions: shown.actions.map { stateAction($0, agent: agent) },
            base: .window
        )
        // Only a critical banner is a new view when its title changes, so that it is
        // announced again; the other rows keep the focus on the button just pressed.
        .id(shown.tone == .critical ? shown.title : "banner")
        .accessibilityIdentifier(AccessibilityID.DaemonBanner.container)
    }

    private func stateAction(_ action: DaemonBannerAction, agent: BackgroundAgentModel?)
        -> StateAction
    {
        switch action {
        case .reconnect:
            return StateAction(
                title: action.title,
                mutates: false,
                isBusy: isReconnecting,
                perform: reconnect
            )
        case .openDiagnostics:
            return StateAction(title: action.title, mutates: false, perform: openDiagnostics)
        case .turnOn:
            return agentAction(action, agent) { await $0.turnOn() }
        case .startDaemon:
            return agentAction(action, agent) { await $0.start() }
        case .checkAgain:
            return agentAction(action, agent) { await $0.refresh() }
        case .openLoginItems:
            return StateAction(
                title: action.title, mutates: false, isBusy: agent?.isWorking ?? false
            ) {
                agent?.openLoginItems()
            }
        }
    }

    private func agentAction(
        _ action: DaemonBannerAction,
        _ agent: BackgroundAgentModel?,
        _ run: @escaping @MainActor (BackgroundAgentModel) async -> Void
    ) -> StateAction {
        StateAction(title: action.title, mutates: false, isBusy: agent?.isWorking ?? false) {
            guard let agent else { return }
            Task { await run(agent) }
        }
    }
}
