import StacklyClient
import SwiftUI

/// The Activity tab of the detail page (spec 0035, AC-15, *Teks Activity*): a native table of
/// Time, By, and Activity from `audit.list { instanceId }`, newest first, with Show More under
/// it. A double click, or Show in Operations in the context menu, opens the operation of a row
/// on the Operations page (AC-13). The tab loads when it appears, after the operations of the
/// instance change state, and after each snapshot round of the shell; the rows stay in the
/// model while another tab shows.
struct InstanceActivityTab: View {
    @Environment(\.shellNavigator) private var navigator
    @Environment(\.loadingDelay) private var loadingDelay
    @Environment(\.fixtureNow) private var fixtureNow
    @Environment(\.calendar) private var calendar
    @Environment(\.locale) private var locale
    @Environment(\.timeZone) private var timeZone
    @State private var selection: UUID?

    /// The tab's model.
    let model: InstanceActivityModel
    /// The operation center of the window, whose changes load the page again, or `nil`.
    let operations: OperationCenter?
    /// How many snapshot rounds the shell completed; each rise loads the page again.
    let completedRounds: Int

    var body: some View {
        content
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
            .onAppear { model.open(changes: operations?.changes()) }
            .onDisappear { model.close() }
            .onChange(of: completedRounds) { model.reconnected() }
    }

    // MARK: - Content

    /// What the tab shows.
    enum Display: Equatable {
        /// The first page has not arrived.
        case loading
        /// The daemon has no `audit.list`.
        case unsupported
        /// The first page failed, with why.
        case failed(ErrorBody)
        /// The instance has no rows.
        case empty
        /// The rows.
        case rows
    }

    /// What the tab of `model` shows: "Activity Unavailable" for an older daemon, the rows
    /// once a page loaded (a later failure keeps them under a stale banner), and "Couldn’t
    /// Load Activity" or the spinner before that.
    static func display(of model: InstanceActivityModel) -> Display {
        if model.unsupported { return .unsupported }
        guard model.loadedAt != nil else {
            if let error = model.error { return .failed(error) }
            return .loading
        }
        return model.items.isEmpty ? .empty : .rows
    }

    @ViewBuilder private var content: some View {
        switch Self.display(of: model) {
        case .loading:
            DelayedProgress(delay: loadingDelay ?? DelayedVisibility.defaultDelay)
                .frame(maxWidth: .infinity, minHeight: 240)
        case .unsupported:
            StatePlaceholder(
                UnsupportedContent(
                    title: ActivityCopy.unsupportedTitle, reason: ActivityCopy.unsupportedReason),
                size: .section)
        case .failed(let body):
            StatePlaceholder(
                FailureContent(
                    title: ActivityCopy.failedTitle, message: DaemonText.sentence(body.message),
                    code: body.code),
                size: .section,
                retry: { Task { await model.reload() } },
                openDiagnostics: { navigator?.show(.diagnostics) })
        case .empty:
            VStack(spacing: Spacing.s12) {
                staleBanner
                StatePlaceholder(
                    .noData(
                        symbol: "clock.arrow.circlepath", title: ActivityCopy.emptyTitle,
                        message: ActivityCopy.emptyMessage),
                    size: .section)
            }
        case .rows:
            VStack(alignment: .leading, spacing: Spacing.s12) {
                staleBanner
                table
                showMore
                Text(ActivityCopy.footer)
                    .font(.caption)
                    .foregroundStyle(StacklyColor.textSecondary)
            }
        }
    }

    /// The caution banner while the rows on screen are from a load before the one that
    /// failed.
    @ViewBuilder private var staleBanner: some View {
        if let loadedAt = model.loadedAt, let error = model.error {
            StaleBanner(
                note: StaleNote(
                    since: loadedAt,
                    reason: .refreshFailed(
                        FailureContent(
                            title: ActivityCopy.failedTitle,
                            message: DaemonText.sentence(error.message), code: error.code))),
                isRefreshing: model.isRefreshing,
                retry: { Task { await model.reload() } })
        }
    }

    // MARK: - Table

    private var table: some View {
        let rows = ActivityCopy.rows(
            model.items, now: fixtureNow ?? Date(), calendar: calendar, locale: locale,
            timeZone: timeZone)
        return Table(rows, selection: $selection) {
            TableColumn("Time") { row in
                Text(row.time)
                    .monospacedDigit()
                    .foregroundStyle(StacklyColor.textSecondary)
                    .help(row.fullTime)
            }
            .width(min: 56, ideal: 84, max: 140)
            TableColumn("By") { row in
                Text(row.by)
                    .fontWeight(.medium)
            }
            .width(min: 52, ideal: 72, max: 120)
            TableColumn("Activity") { row in
                Text(row.activity)
                    .foregroundStyle(
                        row.isFailure ? AnyShapeStyle(Tone.critical.text) : AnyShapeStyle(.primary)
                    )
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
        .contextMenu(forSelectionType: UUID.self) { ids in
            menu(for: ids)
        } primaryAction: { ids in
            if let operation = model.operation(for: ids) {
                navigator?.openOperation(operation)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    @ViewBuilder private func menu(for ids: Set<UUID>) -> some View {
        if let operation = model.operation(for: ids) {
            ActionMenuItems([
                [
                    StacklyAction(
                        id: "activity.showOperation", title: ActivityCopy.showInOperations,
                        mutates: false,
                        perform: { navigator?.openOperation(operation) })
                ]
            ])
        }
    }

    @ViewBuilder private var showMore: some View {
        if model.next != nil {
            HStack(spacing: Spacing.s8) {
                Button("Show More") {
                    Task { await model.showMore() }
                }
                .disabled(model.isLoadingMore)
                if model.isLoadingMore {
                    DelayedProgress()
                } else if let error = model.moreError {
                    Text("Couldn’t load more. \(DaemonText.sentence(error.message))")
                        .font(.callout)
                        .foregroundStyle(StacklyColor.textSecondary)
                }
            }
        }
    }
}
