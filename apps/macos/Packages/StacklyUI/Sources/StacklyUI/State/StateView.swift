import SwiftUI

/// Shows a ``ViewState``: a delayed spinner, an empty or failed or unsupported placeholder, or the content, with stale and partial banners above empty and loaded content.
/// The content is never dimmed or replaced by a spinner during a refresh.
public struct StateView<Value, Content: View>: View {
    @Environment(\.loadingDelay) private var loadingDelay

    private let state: ViewState<Value>
    private let retry: @MainActor () -> Void
    private let openDiagnostics: @MainActor () -> Void
    private let content: (Value) -> Content

    /// Creates a state view.
    /// - Parameters:
    ///   - state: The state, from ``ViewState/resolve(unsupported:value:error:lastLoadedAt:isRefreshing:emptyContent:outdated:partial:)``.
    ///   - retry: Loads again, from the failed placeholder or the stale banner. Clear the
    ///     error first so the page returns to loading.
    ///   - openDiagnostics: Opens Diagnostics, from the failed placeholder.
    ///   - content: The content for loaded data.
    public init(
        state: ViewState<Value>,
        retry: @escaping @MainActor () -> Void,
        openDiagnostics: @escaping @MainActor () -> Void,
        @ViewBuilder content: @escaping (Value) -> Content
    ) {
        self.state = state
        self.retry = retry
        self.openDiagnostics = openDiagnostics
        self.content = content
    }

    /// The view's content.
    public var body: some View {
        ZStack {
            switch state {
            case .loading:
                DelayedProgress(delay: loadingDelay ?? DelayedVisibility.defaultDelay)
                    .transition(StacklyMotion.crossfade)
            case .empty(let empty, let notes):
                ScrollView {
                    VStack(spacing: 0) {
                        ContentNotesBanners(notes: notes, retry: retry)
                        StatePlaceholder(empty)
                    }
                }
                .transition(StacklyMotion.crossfade)
            case .failed(let failure):
                ScrollView {
                    StatePlaceholder(failure, retry: retry, openDiagnostics: openDiagnostics)
                }
                .transition(StacklyMotion.crossfade)
            case .unsupported(let unsupported):
                ScrollView {
                    StatePlaceholder(unsupported)
                }
                .transition(StacklyMotion.crossfade)
            case .loaded(let value, let notes):
                VStack(spacing: 0) {
                    ContentNotesBanners(notes: notes, retry: retry)
                    content(value)
                        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
                }
                .transition(StacklyMotion.crossfade)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)

    }
}

/// The stale and partial banners above empty or loaded content, with the page inset.
/// Nothing when there is neither.
private struct ContentNotesBanners: View {
    let notes: ContentNotes
    let retry: @MainActor () -> Void

    var body: some View {
        if notes.stale != nil || notes.partial != nil {
            VStack(spacing: Spacing.s8) {
                if let stale = notes.stale {
                    StaleBanner(note: stale, isRefreshing: notes.isRefreshing, retry: retry)
                        .transition(StacklyMotion.crossfade)
                }
                if let partial = notes.partial {
                    StateBanner(tone: .caution, title: partial.summary, items: partial.items)
                        .transition(StacklyMotion.crossfade)
                }
            }
            .padding(.horizontal, Spacing.pageHorizontal)
            .padding(.vertical, Spacing.s12)
        }
    }
}

/// The caution banner of a stale page: "Showing the last known state from <time>.", why,
/// and Retry, which is disabled with a small spinner while a refresh runs. The time
/// follows the clock, the calendar, the locale, and the time zone of the environment.
struct StaleBanner: View {
    @Environment(\.fixtureNow) private var fixtureNow
    @Environment(\.calendar) private var calendar
    @Environment(\.locale) private var locale
    @Environment(\.timeZone) private var timeZone

    let note: StaleNote
    let isRefreshing: Bool
    let retry: @MainActor () -> Void

    var body: some View {
        TimelineView(.everyMinute) { context in
            StateBanner(
                tone: .caution,
                title: note.bannerTitle(
                    now: fixtureNow ?? context.date,
                    calendar: calendar,
                    locale: locale,
                    timeZone: timeZone
                ),
                message: note.bannerMessage(
                    now: fixtureNow ?? context.date,
                    calendar: calendar,
                    locale: locale,
                    timeZone: timeZone
                ),
                actions: [Self.retryAction(isRefreshing: isRefreshing, retry: retry)]
            )
        }
    }

    /// Retry: it does not mutate, so it stays enabled while mutations are blocked, and it
    /// is busy while a refresh runs.
    static func retryAction(
        isRefreshing: Bool,
        retry: @escaping @MainActor () -> Void
    ) -> StateAction {
        StateAction(title: "Retry", mutates: false, isBusy: isRefreshing, perform: retry)
    }
}

extension EnvironmentValues {
    /// The moment views treat as now when they show a time relative to it, or `nil` for
    /// the real clock. Fixtures and snapshots set it so a rendered time never moves; the
    /// running app leaves it `nil` (spec 0068).
    @Entry public var fixtureNow: Date? = nil

    /// How long ``StateView`` waits before it shows its spinner, or `nil` for the default
    /// 300 ms. Fixtures and snapshots set it to zero so a loading state always shows the
    /// spinner (spec 0068).
    @Entry public var loadingDelay: Duration? = nil
}
