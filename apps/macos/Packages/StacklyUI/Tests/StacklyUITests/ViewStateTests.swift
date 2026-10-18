import Foundation
import Testing

@testable import StacklyUI

/// `ViewState.resolve` and the stale note (spec 0012, State patterns; AC-8, AC-12, AC-14).
struct ViewStateTests {
    /// One combination of inputs and the shape `resolve` must pick for it.
    nonisolated struct OrderCase: CustomTestStringConvertible, Sendable {
        /// What the view model holds as its last loaded value.
        nonisolated enum Value: Sendable {
            case none
            case empty
            case items
        }

        let isUnsupported: Bool
        let value: Value
        let hasError: Bool
        let expected: ViewStateShape

        var testDescription: String {
            "unsupported \(isUnsupported), value \(value), error \(hasError) → \(expected)"
        }
    }

    nonisolated static let orderCases = [
        OrderCase(isUnsupported: true, value: .none, hasError: false, expected: .unsupported),
        OrderCase(isUnsupported: true, value: .items, hasError: true, expected: .unsupported),
        OrderCase(isUnsupported: true, value: .none, hasError: true, expected: .unsupported),
        OrderCase(isUnsupported: false, value: .none, hasError: true, expected: .failed),
        OrderCase(isUnsupported: false, value: .none, hasError: false, expected: .loading),
        OrderCase(isUnsupported: false, value: .empty, hasError: false, expected: .empty),
        OrderCase(isUnsupported: false, value: .empty, hasError: true, expected: .empty),
        OrderCase(isUnsupported: false, value: .items, hasError: false, expected: .loaded),
        OrderCase(isUnsupported: false, value: .items, hasError: true, expected: .loaded),
    ]

    static let loadedAt = Date(timeIntervalSince1970: 1_792_245_720)  // 2026-10-17 14:02 UTC
    static let failure = FailureContent(
        title: "Couldn’t Load Instances",
        message: "the registry is busy",
        code: "E_REGISTRY_BUSY"
    )

    /// No data when the list is empty, otherwise nothing.
    static func noData(_ value: [String]) -> EmptyContent? {
        value.isEmpty
            ? .noData(symbol: "server.rack", title: "No Instances", message: "Install a runtime.")
            : nil
    }

    @Test(arguments: orderCases)
    func resolveFollowsTheFixedOrder(order: OrderCase) {
        let value: [String]? =
            switch order.value {
            case .none: nil
            case .empty: []
            case .items: ["pg17-dev"]
            }
        let state = ViewState.resolve(
            unsupported: order.isUnsupported
                ? UnsupportedContent(title: "Backups Aren’t Supported", reason: "No capability.")
                : nil,
            value: value,
            error: order.hasError ? Self.failure : nil,
            lastLoadedAt: value == nil ? nil : Self.loadedAt,
            emptyContent: Self.noData
        )
        #expect(state.shape == order.expected)
    }

    @Test func pageNeverShowsEmptyBeforeDataLoaded() {
        var askedForEmptyContent = false
        let state = ViewState<[String]>.resolve(
            value: nil,
            error: nil,
            lastLoadedAt: nil,
            emptyContent: { _ in
                askedForEmptyContent = true
                return .noData(symbol: "server.rack", title: "No Instances", message: "")
            }
        )
        #expect(state.shape == .loading)
        #expect(askedForEmptyContent == false)
    }

    @Test func firstLoadFailureCarriesTheDaemonError() throws {
        let state = ViewState<[String]>.resolve(
            value: nil,
            error: Self.failure,
            lastLoadedAt: nil,
            emptyContent: Self.noData
        )
        guard case .failed(let failure) = state else {
            Issue.record("expected failed, got \(state.shape)")
            return
        }
        #expect(failure == Self.failure)
    }

    @Test func retryFromFailedReturnsToLoading() {
        var model = FakeViewModel()
        model.error = Self.failure
        #expect(model.state.shape == .failed)

        model.retry()

        #expect(model.state.shape == .loading)
    }

    @Test func failedRefreshKeepsTheDataWithAStaleNote() throws {
        let state = ViewState.resolve(
            value: ["pg17-dev"],
            error: Self.failure,
            lastLoadedAt: Self.loadedAt,
            emptyContent: Self.noData
        )
        guard case .loaded(let value, let notes) = state else {
            Issue.record("expected loaded, got \(state.shape)")
            return
        }
        #expect(value == ["pg17-dev"])
        let stale = try #require(notes.stale)
        #expect(stale == StaleNote(since: Self.loadedAt, reason: .refreshFailed(Self.failure)))
    }

    @Test func failedRefreshOfEmptyDataStaysEmptyWithAStaleNote() throws {
        let state = ViewState.resolve(
            value: [String](),
            error: Self.failure,
            lastLoadedAt: Self.loadedAt,
            emptyContent: Self.noData
        )
        guard case .empty(let empty, let notes) = state else {
            Issue.record("expected empty, got \(state.shape)")
            return
        }
        #expect(empty.kind == .noData)
        #expect(notes.stale?.reason == .refreshFailed(Self.failure))
    }

    @Test func failedRefreshWinsOverOutdated() {
        let state = ViewState.resolve(
            value: ["pg17-dev"],
            error: Self.failure,
            lastLoadedAt: Self.loadedAt,
            emptyContent: Self.noData,
            outdated: (since: Self.loadedAt.addingTimeInterval(-3600), source: "Catalog")
        )
        #expect(
            state.notes?.stale
                == StaleNote(since: Self.loadedAt, reason: .refreshFailed(Self.failure)))
    }

    @Test func outdatedDataGetsAnOutdatedNoteFromItsSource() {
        let updated = Self.loadedAt.addingTimeInterval(-86_400)
        let state = ViewState.resolve(
            value: ["pg17-dev"],
            error: nil,
            lastLoadedAt: Self.loadedAt,
            emptyContent: Self.noData,
            outdated: (since: updated, source: "Catalog")
        )
        #expect(
            state.notes?.stale == StaleNote(since: updated, reason: .outdated(source: "Catalog")))
    }

    @Test func freshDataHasNoStaleNote() {
        let state = ViewState.resolve(
            value: ["pg17-dev"],
            error: nil,
            lastLoadedAt: Self.loadedAt,
            emptyContent: Self.noData
        )
        #expect(state.shape == .loaded)
        #expect(state.notes?.stale == nil)
        #expect(state.notes?.partial == nil)
        #expect(state.notes?.isRefreshing == false)
    }

    @Test func partialAndRefreshingPassThroughToTheNotes() throws {
        let partial = PartialNote(
            summary: "5 of 6 services ready",
            items: [PartialItem(name: "valkey-erp", reason: "readiness failed (PING timeout)")]
        )
        let state = ViewState.resolve(
            value: ["pg18-dev", "valkey-erp"],
            error: nil,
            lastLoadedAt: Self.loadedAt,
            isRefreshing: true,
            emptyContent: Self.noData,
            partial: partial
        )
        let notes = try #require(state.notes)
        #expect(notes.isRefreshing)
        #expect(notes.partial?.summary == "5 of 6 services ready")
        #expect(notes.partial?.items.map(\.name) == ["valkey-erp"])
    }

    @Test func filterThatLeavesNothingIsNoResultsAndKeepsTheData() {
        let instances = ["pg17-dev", "mysql-erp"]
        let query = "redis"
        let state = ViewState.resolve(
            value: instances,
            error: nil,
            lastLoadedAt: Self.loadedAt,
            emptyContent: { value in
                if value.isEmpty { return Self.noData(value) }
                let filtered = value.filter { $0.contains(query) }
                return filtered.isEmpty
                    ? .noResults(query: query, message: "Check the spelling.", clearFilters: {})
                    : nil
            }
        )
        guard case .empty(let empty, _) = state else {
            Issue.record("expected empty, got \(state.shape)")
            return
        }
        #expect(empty.kind == .noResults(query: "redis"))
    }
}

/// The stale note's time and banner copy (spec 0012, State patterns; AC-12).
struct StaleNoteTests {
    static let utc = TimeZone.gmt
    static let gregorian = Calendar(identifier: .gregorian)
    static let unitedStates = Locale(identifier: "en_US")
    static let britain = Locale(identifier: "en_GB")

    /// October `day`, 2026 at `hour`:`minute` UTC.
    static func october(_ day: Int, _ hour: Int, _ minute: Int) -> Date {
        let midnight = Date(timeIntervalSince1970: 1_792_281_600)  // 2026-10-18 00:00 UTC
        let seconds = (day - 18) * 86_400 + hour * 3600 + minute * 60
        return midnight.addingTimeInterval(TimeInterval(seconds))
    }

    static func note(since: Date, reason: StaleNote.Reason = .outdated(source: "Catalog"))
        -> StaleNote
    {
        StaleNote(since: since, reason: reason)
    }

    @Test func todayShowsOnlyTheShortTimeInTheLocaleStyle() {
        let note = Self.note(since: Self.october(18, 14, 2))
        let now = Self.october(18, 18, 0)
        #expect(
            note.formattedSince(
                now: now, calendar: Self.gregorian, locale: Self.unitedStates, timeZone: Self.utc)
                == "2:02\u{202F}PM"
        )
        #expect(
            note.formattedSince(
                now: now, calendar: Self.gregorian, locale: Self.britain, timeZone: Self.utc)
                == "14:02"
        )
    }

    @Test func anotherDayAddsTheShortDate() {
        let note = Self.note(since: Self.october(17, 14, 2))
        let now = Self.october(18, 9, 30)
        #expect(
            note.formattedSince(
                now: now, calendar: Self.gregorian, locale: Self.unitedStates, timeZone: Self.utc)
                == "10/17/2026, 2:02\u{202F}PM"
        )
        #expect(
            note.formattedSince(
                now: now, calendar: Self.gregorian, locale: Self.britain, timeZone: Self.utc)
                == "17/10/2026, 14:02"
        )
    }

    @Test func theTimeZoneDecidesWhatTodayIs() throws {
        let since = Self.october(17, 23, 30)
        let now = Self.october(18, 0, 30)
        let note = Self.note(since: since)
        let minusTwo = try #require(TimeZone(secondsFromGMT: -2 * 3600))
        #expect(
            note.formattedSince(
                now: now, calendar: Self.gregorian, locale: Self.britain, timeZone: Self.utc)
                == "17/10/2026, 23:30"
        )
        #expect(
            note.formattedSince(
                now: now, calendar: Self.gregorian, locale: Self.britain, timeZone: minusTwo)
                == "21:30"
        )
    }

    @Test func bannerTitleNamesTheLastKnownTime() {
        let note = Self.note(since: Self.october(18, 14, 2))
        #expect(
            note.bannerTitle(
                now: Self.october(18, 18, 0),
                calendar: Self.gregorian,
                locale: Self.britain,
                timeZone: Self.utc
            ) == "Showing the last known state from 14:02."
        )
    }

    @Test func failedRefreshMessageQuotesTheDaemonMessageOnce() {
        let now = Self.october(18, 18, 0)
        let bare = Self.note(
            since: Self.october(18, 14, 2),
            reason: .refreshFailed(
                FailureContent(title: "Couldn’t Load", message: "the registry is busy"))
        )
        let withPeriod = Self.note(
            since: Self.october(18, 14, 2),
            reason: .refreshFailed(
                FailureContent(title: "Couldn’t Load", message: "The registry is busy."))
        )
        #expect(
            bare.bannerMessage(
                now: now, calendar: Self.gregorian, locale: Self.britain, timeZone: Self.utc)
                == "Couldn’t refresh: the registry is busy."
        )
        #expect(
            withPeriod.bannerMessage(
                now: now, calendar: Self.gregorian, locale: Self.britain, timeZone: Self.utc)
                == "Couldn’t refresh: The registry is busy."
        )
    }

    @Test func outdatedMessageNamesTheSourceAndItsTime() {
        let note = Self.note(since: Self.october(18, 14, 2), reason: .outdated(source: "Catalog"))
        #expect(
            note.bannerMessage(
                now: Self.october(18, 18, 0),
                calendar: Self.gregorian,
                locale: Self.britain,
                timeZone: Self.utc
            ) == "Catalog last updated 14:02."
        )
    }

    @Test func staleRetryIsBusyWhileRefreshingAndNeverGated() {
        var retried = false
        let blocked = MutationAvailability.blocked(reason: "stacklyd is unavailable", since: .now)

        let idle = StaleBanner.retryAction(isRefreshing: false) { retried = true }
        let busy = StaleBanner.retryAction(isRefreshing: true) {}

        #expect(idle.title == "Retry")
        #expect(idle.isBusy == false)
        #expect(busy.isBusy)
        #expect(idle.disabledReason(in: blocked) == nil)
        idle.perform()
        #expect(retried)
    }
}

/// A screen view model that follows the view model contract in `apps/macos/design.md`.
private struct FakeViewModel {
    var value: [String]?
    var lastLoadedAt: Date?
    var error: FailureContent?

    var state: ViewState<[String]> {
        .resolve(
            value: value,
            error: error,
            lastLoadedAt: lastLoadedAt,
            emptyContent: ViewStateTests.noData
        )
    }

    /// Retry from failed: clear the error, then load again.
    mutating func retry() {
        error = nil
    }
}

extension ViewState {
    /// The notes of `.empty` and `.loaded`, `nil` for the other shapes.
    fileprivate var notes: ContentNotes? {
        switch self {
        case .empty(_, let notes), .loaded(_, let notes): notes
        case .loading, .failed, .unsupported: nil
        }
    }
}
