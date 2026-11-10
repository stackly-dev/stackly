import StacklyClient
import SwiftUI

/// One row of the Operations page (spec 0035, *Halaman Operations*): the title, the note line,
/// "{time} · {short ID}", the badge, Cancel and Retry, and while the operation runs its staged
/// progress, or the stage label with an indeterminate progress for a kind without stages.
struct OperationRow: View {
    @Environment(\.locale) private var locale
    @Environment(\.timeZone) private var timeZone

    let operation: OperationSnapshot
    let title: String
    let actions: [StacklyAction]

    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.s8) {
            ViewThatFits(in: .horizontal) {
                HStack(alignment: .center, spacing: Spacing.s12) {
                    texts
                        .frame(maxWidth: .infinity, alignment: .leading)
                    meta
                    badgeAndButtons
                }
                VStack(alignment: .leading, spacing: Spacing.s6) {
                    texts
                    meta
                    badgeAndButtons
                }
                .frame(maxWidth: .infinity, alignment: .leading)
            }
            if Self.showsProgress(operation) {
                progress
            }
        }
        .padding(.vertical, Spacing.s6)
    }

    private var texts: some View {
        let note = OperationNote.line(for: operation)
        return VStack(alignment: .leading, spacing: Spacing.s2) {
            Text(title)
                .font(.body.weight(.semibold))
                .foregroundStyle(.primary)
                .fixedSize(horizontal: false, vertical: true)
            Text(note.text)
                .font(.callout)
                .foregroundStyle(note.isCritical ? Tone.critical.text : StacklyColor.textSecondary)
                .fixedSize(horizontal: false, vertical: true)
        }
    }

    private var meta: some View {
        Text(OperationsCopy.meta(operation, locale: locale, timeZone: timeZone))
            .font(.callout.monospacedDigit())
            .foregroundStyle(StacklyColor.textSecondary)
            .lineLimit(1)
            .fixedSize()
    }

    private var badgeAndButtons: some View {
        HStack(spacing: Spacing.s8) {
            ToneBadge(OperationCancel.appearance(of: operation))
            ForEach(actions) { action in
                ActionButton(action, showsReason: false)
                    .controlSize(.small)
                    .help(action.disabledReason ?? action.title)
            }
        }
        .fixedSize()
    }

    @ViewBuilder private var progress: some View {
        if let stages = OperationStages.definitions(for: operation.kind) {
            StagedProgress(
                StagedProgressValue.from(operation: operation, stages: stages), layout: .stages)
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

    /// Whether the row shows progress: while the operation runs or winds down.
    static func showsProgress(_ operation: OperationSnapshot) -> Bool {
        operation.state == .running || operation.state == .cancelRequested
    }
}
