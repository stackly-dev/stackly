import SwiftUI

/// The one window banner for the daemon's state, above every page: a ``StateBanner`` on
/// the window surface with the banner's actions.
///
/// Its time follows the clock, the calendar, the locale, and the time zone of the
/// environment, and updates every minute. Reconnect shows a small spinner while an
/// attempt runs. No action is gated, because none changes data.
public struct DaemonBanner: View {
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

    /// The banner's content.
    public var body: some View {
        TimelineView(.everyMinute) { context in
            StateBanner(
                tone: content.tone,
                title: content.title,
                message: content.message(
                    now: fixtureNow ?? context.date,
                    calendar: calendar,
                    locale: locale,
                    timeZone: timeZone
                ),
                actions: content.actions.map(stateAction),
                base: .window
            )
            .accessibilityIdentifier(AccessibilityID.DaemonBanner.container)
        }
    }

    private func stateAction(_ action: DaemonBannerAction) -> StateAction {
        switch action {
        case .reconnect:
            StateAction(
                title: action.title,
                mutates: false,
                isBusy: isReconnecting,
                perform: reconnect
            )
        case .openDiagnostics:
            StateAction(title: action.title, mutates: false, perform: openDiagnostics)
        }
    }
}
