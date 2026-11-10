import Foundation
import StacklyClient
import Testing

@testable import StacklyUI

/// What the Operations page shows before and around its rows (spec 0035 AC-11, AC-12): the
/// placeholders of an older daemon, a first load that failed, a daemon that cannot be
/// reached, and an empty history, and the stale note that keeps the last rows on screen when
/// a later load fails, as the shell rules of spec 0013 ask. The words come from the spec.
@MainActor
struct OperationsPageStateTests {
    private static let failure = ErrorBody(
        code: "internal.unexpected", category: .internal, message: "the registry could not be read",
        retryable: false, resource: nil, fields: nil, hint: nil, details: nil, operationId: nil,
        rule: nil, items: nil)

    private static let waiting = FailureContent(
        title: "Waiting for stacklyd", message: "Stackly can’t reach stacklyd.", code: nil)

    private func state(
        rows: [OperationSnapshot]?, error: ErrorBody? = nil, unsupported: Bool = false,
        waiting: FailureContent? = nil
    ) -> ViewState<[OperationSnapshot]> {
        OperationsPage.state(
            rows: rows, error: error, unsupported: unsupported, waiting: waiting,
            loadedAt: rows == nil ? nil : FixtureClock.now, isRefreshing: false)
    }

    private var rows: [OperationSnapshot] {
        [OperationSample.make(1, state: .running), OperationSample.make(2, state: .failed)]
    }

    @Test(
        .evidence(
            covers: ["REQ-054"],
            expected:
                "an older daemon without operation.list shows Operations Unavailable with its reason, whatever else the page holds"
        ))
    func anOlderDaemonShowsOperationsUnavailable() {
        for held in [nil, rows] {
            guard case .unsupported(let content) = state(rows: held, unsupported: true) else {
                Issue.record("expected unsupported")
                return
            }
            #expect(content.title == "Operations Unavailable")
            #expect(content.reason == OperationsCopy.unsupportedReason)
            #expect(
                content.reason
                    == "This version of stacklyd doesn’t list operations. Update Stackly.")
        }
    }

    @Test func aFirstLoadThatFailedShowsCouldntLoadOperationsWithTheDaemonsWords() {
        guard case .failed(let content) = state(rows: nil, error: Self.failure) else {
            Issue.record("expected failed")
            return
        }
        #expect(content.title == "Couldn’t Load Operations")
        #expect(content.message == DaemonText.sentence(Self.failure.message))
        #expect(content.code == "internal.unexpected")
    }

    @Test func aDaemonThatCannotBeReachedBeforeAnyLoadShowsTheWaitingContent() {
        guard
            case .failed(let content) = state(
                rows: nil, error: Self.failure, waiting: Self.waiting)
        else {
            Issue.record("expected the waiting content")
            return
        }
        #expect(content == Self.waiting, "the shell's words win over the load error")
        guard case .loading = state(rows: nil) else {
            Issue.record("expected loading")
            return
        }
    }

    @Test(
        .evidence(
            covers: ["REQ-054"],
            expected:
                "a history with no operations shows No Operations with the spec's message, and a loaded one shows its rows without a stale note"
        ))
    func anEmptyHistoryShowsNoOperationsAndALoadedOneItsRows() {
        guard case .empty(let content, let notes) = state(rows: []) else {
            Issue.record("expected empty")
            return
        }
        #expect(content.title == "No Operations")
        #expect(content.message == OperationsCopy.emptyMessage)
        #expect(content.symbol == SidebarSection.operations.style.symbol)
        #expect(notes.stale == nil)

        guard case .loaded(let shown, let notes) = state(rows: rows) else {
            Issue.record("expected loaded")
            return
        }
        #expect(shown.map(\.id) == rows.map(\.id))
        #expect(notes.stale == nil)
    }

    @Test(
        .evidence(
            covers: ["REQ-054"],
            expected:
                "once the history loaded, a later failure or a lost daemon keeps the last rows on screen with a stale note instead of a placeholder"
        ))
    func aLaterFailureKeepsTheLastRowsWithAStaleNote() {
        for waiting in [nil, Self.waiting] {
            guard
                case .loaded(let shown, let notes) = state(
                    rows: rows, error: Self.failure, waiting: waiting)
            else {
                Issue.record("expected the last rows")
                return
            }
            #expect(shown.count == 2)
            #expect(notes.stale != nil)
        }
    }

    @Test func anOperationTheDaemonNoLongerKnowsHasTheWordsOfTheSpec() {
        #expect(OperationsCopy.noLongerInHistory == "This operation is no longer in the history.")
    }
}
