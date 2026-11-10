import StacklyClient
import SwiftUI

/// The inspector of the Operations page (spec 0035, AC-12): the header with the operation's
/// buttons, then what `operation.get` tells: the kind, resource, who started it and from
/// where, its times, the full ID with Copy, the steps, the items it created and what is left of
/// them, the failure, the recovery, and a partial result. An operation the daemon no longer
/// knows shows only that it is gone from the history.
struct OperationInspector: View {
    @Environment(\.locale) private var locale
    @Environment(\.timeZone) private var timeZone

    /// The operation as the list shows it, or `nil` while it is read for the first time.
    let operation: OperationSnapshot?
    let phase: OperationDetailModel.Phase
    let title: String
    let actions: [StacklyAction]
    /// The names of the instances and runtimes by ID, for the resource.
    let names: [UUID: String]
    let retry: @MainActor () -> Void

    var body: some View {
        switch phase {
        case .missing:
            InspectorPlaceholder(
                title: "Not in History", message: OperationsCopy.noLongerInHistory)
        case .failed(let error) where summary == nil:
            StateBanner(
                tone: .critical, title: "Couldn’t Load Operation",
                message: DaemonText.sentence(error.message), code: error.code,
                actions: [StateAction(title: "Retry", mutates: false, perform: retry)])
        default:
            if let summary {
                content(summary)
            } else {
                DelayedProgress()
                    .frame(maxWidth: .infinity)
            }
        }
    }

    /// The newest summary the page holds: the list's, or the detail's when it is newer.
    private var summary: OperationSnapshot? {
        guard case .loaded(let detail) = phase else { return operation }
        guard let operation, operation.id == detail.operation.id else { return detail.operation }
        return detail.operation.revision > operation.revision ? detail.operation : operation
    }

    private var detail: OperationDetail? {
        if case .loaded(let detail) = phase { return detail }
        return nil
    }

    @ViewBuilder
    private func content(_ summary: OperationSnapshot) -> some View {
        InspectorHeader(
            title: title, subtitle: OperationsCopy.kind(summary.kind),
            status: OperationCancel.appearance(of: summary)
        ) {
            ForEach(actions) { action in
                ActionButton(action, showsReason: true)
            }
        }
        banner(summary)
        CardSection {
            LabeledContent("Kind") { Text(OperationsCopy.kind(summary.kind)) }
            LabeledContent("Resource") {
                Text(OperationsCopy.resource(summary.resource, names: names))
            }
            LabeledContent("By") { Text(OperationsCopy.by(summary.trigger)) }
            LabeledContent("Created") { Text(moment(summary.createdAt)) }
            if let started = summary.startedAt {
                LabeledContent("Started") { Text(moment(started)) }
            }
            LabeledContent("Updated") { Text(moment(summary.updatedAt)) }
            if let cancel = summary.cancelRequestedAt {
                LabeledContent("Cancel asked") { Text(moment(cancel)) }
            }
            if let point = summary.pointOfNoReturnAt {
                LabeledContent("Can’t undo since") { Text(moment(point)) }
            }
            idRow(summary.id)
        }
        .labeledContentStyle(.stackly)
        if let steps = detail?.steps, steps.isEmpty == false {
            CardSection("Steps") {
                ForEach(steps, id: \.seq) { step in
                    LabeledContent(OperationsCopy.sentence(step.name)) {
                        Text(OperationsCopy.stepLine(step, locale: locale, timeZone: timeZone))
                    }
                }
            }
            .labeledContentStyle(.stackly)
        }
        if let created = detail?.created, created.isEmpty == false {
            CardSection("Created Items") {
                ForEach(Array(created.enumerated()), id: \.offset) { _, item in
                    LabeledContent(OperationsCopy.sentence(item.kind)) {
                        VStack(alignment: .trailing, spacing: Spacing.s2) {
                            Text(OperationsCopy.created(item.state))
                            Text(item.reference)
                                .font(.callout.monospaced())
                        }
                    }
                }
            }
            .labeledContentStyle(.stackly)
        }
        if let failure = summary.failure {
            CardSection("Failure") {
                LabeledContent("Code") { Text(failure.error.code).font(.callout.monospaced()) }
                LabeledContent("Reason") { Text(OperationsCopy.sentence(failure.reason)) }
                if let hint = failure.error.hint {
                    LabeledContent("Hint") { Text(DaemonText.sentence(hint)) }
                }
                ForEach(Array((failure.error.items ?? []).enumerated()), id: \.offset) { _, item in
                    LabeledContent("Item") {
                        Text(
                            ([item.kind] + (item.fields ?? []).map { "\($0.key) \($0.value)" })
                                .joined(separator: " · "))
                    }
                }
            }
            .labeledContentStyle(.stackly)
        }
        if let recovery = summary.recovery {
            CardSection("Recovery") {
                LabeledContent("Cause") { Text(OperationsCopy.cause(recovery.cause)) }
                LabeledContent("Next") { Text(OperationNote.recovery(recovery.hint)) }
                LabeledContent("Actions") { Text(OperationsCopy.actions(recovery.actions)) }
                if let error = recovery.error {
                    LabeledContent("Code") { Text(error.code).font(.callout.monospaced()) }
                }
            }
            .labeledContentStyle(.stackly)
        }
        if let partial = summary.result?.partial, partial.isEmpty == false {
            CardSection("Result") {
                ForEach(Array(partial.enumerated()), id: \.offset) { _, item in
                    LabeledContent(item.item) {
                        Text(
                            ([OperationsCopy.outcome(item.outcome)] + (item.code.map { [$0] } ?? []))
                                .joined(separator: " · "))
                    }
                }
            }
            .labeledContentStyle(.stackly)
        }
    }

    /// The failure or the recovery, in words, above the facts.
    @ViewBuilder
    private func banner(_ summary: OperationSnapshot) -> some View {
        if summary.state == .failed, let error = summary.failure?.error {
            StateBanner(
                tone: .critical, title: "Failed", message: DaemonText.sentence(error.message),
                code: error.code)
        } else if summary.state == .needsRecovery {
            StateBanner(
                tone: .critical, title: "Needs Recovery",
                message: OperationNote.recovery(summary.recovery?.hint),
                code: summary.recovery?.error?.code)
        }
    }

    private func idRow(_ id: UUID) -> some View {
        let text = id.uuidString.lowercased()
        return HStack(spacing: Spacing.s8) {
            LabeledContent("ID") {
                Text(text).font(.callout.monospaced())
            }
            CopyButton(text, what: "operation ID", style: .icon)
        }
    }

    private func moment(_ date: Date) -> String {
        OperationsCopy.moment(date, locale: locale, timeZone: timeZone)
    }
}
