import StacklyClient
import SwiftUI

/// What the Operations section of the Overview shows (spec 0022, *Seksi Operations*).
enum OverviewOperationsContent {
    /// The first load has not ended.
    case loading
    /// The load failed and there is nothing to show.
    case failed(title: String, retry: @MainActor () -> Void)
    /// The daemon does not know the method.
    case unsupported
    /// The operations to show, newest first, at most five, and how many there are.
    case loaded(rows: [OperationSnapshot], total: Int)
}

/// The Operations section: the operations that queue, run, or are being cancelled, newest
/// first. An operation that needs recovery is an item of Needs Attention instead.
struct OverviewOperations: View {
    let content: OverviewOperationsContent
    let title: (OperationSnapshot) -> String
    let open: @MainActor () -> Void
    let openDiagnostics: @MainActor () -> Void

    var body: some View {
        switch content {
        case .loading:
            SectionCard(title: "Operations") {
                SectionProgress()
            }
        case .failed(_, let retry):
            VStack(alignment: .leading, spacing: Spacing.s6) {
                Text("Operations").font(.headline)
                StatePlaceholder(
                    FailureContent(
                        title: "Couldn’t Load Operations",
                        message: "Stackly couldn’t read the operations from stacklyd."),
                    size: .section, retry: retry, openDiagnostics: openDiagnostics
                )
                .frame(maxWidth: .infinity)
                .cardSurface()
            }
        case .unsupported:
            VStack(alignment: .leading, spacing: Spacing.s6) {
                Text("Operations").font(.headline)
                StatePlaceholder(
                    UnsupportedContent(
                        title: "Operations Unavailable",
                        reason: "This version of stacklyd doesn’t list operations. Update Stackly."),
                    size: .section
                )
                .frame(maxWidth: .infinity)
                .cardSurface()
            }
        case .loaded(let rows, let total):
            SectionCard(title: "Operations") {
                if rows.isEmpty {
                    InlineEmptyRow(OverviewCopy.nothingRunning)
                } else {
                    ForEach(rows) { operation in
                        OverviewOperationRow(
                            operation: operation, title: title(operation), open: open)
                    }
                    if total > rows.count {
                        Button(action: open) {
                            Text(OverviewCopy.showAllOperations)
                                .frame(maxWidth: .infinity, alignment: .leading)
                                .padding(Spacing.rowInsets)
                                .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                        .foregroundStyle(Tone.informative.text)
                    }
                }
            }
        }
    }
}

/// One running operation: the title, the state badge, and the progress.
struct OverviewOperationRow: View {
    let operation: OperationSnapshot
    let title: String
    let open: @MainActor () -> Void

    var body: some View {
        Button(action: open) {
            VStack(alignment: .leading, spacing: Spacing.s6) {
                HStack(alignment: .firstTextBaseline, spacing: Spacing.s8) {
                    Text(title)
                        .font(.body.weight(.semibold))
                        .foregroundStyle(.primary)
                        .frame(maxWidth: .infinity, alignment: .leading)
                    ToneBadge(StatusAdapter.appearance(of: operation))
                }
                progress
            }
            .padding(Spacing.rowInsets)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)

    }

    @ViewBuilder private var progress: some View {
        if let stages = OperationStages.definitions(for: operation.kind) {
            StagedProgress(
                StagedProgressValue.from(operation: operation, stages: stages), layout: .compact)
        } else {
            HStack(spacing: Spacing.s8) {
                Text(OperationStages.label(for: operation))
                    .font(.callout)
                    .foregroundStyle(StacklyColor.textSecondary)
                ProgressView()
                    .controlSize(.small)

            }
        }
    }

}
