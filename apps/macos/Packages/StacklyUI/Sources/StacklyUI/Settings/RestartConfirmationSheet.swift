import StacklyClient
import SwiftUI

/// The sheet Restart Daemon opens (spec 0030 AC-16): which instances keep running, which
/// stop and start again, and which operations are in flight, so the person knows how far the
/// disruption reaches before they confirm. The plan is the daemon's, so a plan that changed
/// while the sheet was open is shown again instead of confirmed blind.
struct RestartConfirmationSheet: View {
    let restart: DaemonRestartModel

    var body: some View {
        if let confirmation = restart.confirmation {
            StacklySheet(
                title: RestartCopy.buttonTitle,
                subtitle: "stacklyd restarts. The instances below are affected as listed.",
                primary: StacklyAction(
                    id: "settings.daemon.restart.confirm",
                    title: "Restart",
                    mutates: true,
                    disabledReason: confirmation.preview.launchContext == .manual
                        ? RestartCopy.manualSubtitle : nil,
                    isBusy: confirmation.isWorking,
                    perform: { Task { await restart.confirm() } }),
                cancel: StacklyAction(
                    id: "settings.daemon.restart.cancel",
                    title: "Cancel",
                    mutates: false,
                    perform: { restart.cancel() }),
                minWidth: 520
            ) {
                RestartPlanContent(confirmation: confirmation)
            }
        }
    }
}

/// The body of the sheet: notes, the two instance lists, and the operations in flight.
struct RestartPlanContent: View {
    let confirmation: DaemonRestartModel.Confirmation

    private var preview: RestartPreview { confirmation.preview }
    private var instances: [RestartPreviewInstance] { preview.instances ?? [] }
    private var keeping: [RestartPreviewInstance] { instances.filter { $0.decision == .keep } }
    private var stopping: [RestartPreviewInstance] {
        instances.filter { $0.decision == .stopRestore }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.s16) {
            if confirmation.planChanged {
                StateBanner(tone: .caution, title: RestartCopy.planChanged)
            }
            if let error = confirmation.error {
                StateBanner(
                    tone: .critical, title: DaemonText.sentence(error.message),
                    message: error.hint.map(DaemonText.sentence), code: error.code)
            }
            if confirmation.isWorking {
                HStack(spacing: Spacing.s8) {
                    ProgressView().controlSize(.small)
                    Text(RestartCopy.waitingForOperations)
                        .foregroundStyle(StacklyColor.textSecondary)
                }
            }
            if instances.isEmpty {
                Text("No instance is running. Only stacklyd restarts.")
                    .foregroundStyle(StacklyColor.textSecondary)
            }
            group("Keeps running", rows: keeping)
            group("Stops, then starts again", rows: stopping)
            operations
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    @ViewBuilder private func group(_ title: String, rows: [RestartPreviewInstance]) -> some View {
        if rows.isEmpty == false {
            VStack(alignment: .leading, spacing: Spacing.s8) {
                Text(title).font(.headline)
                ForEach(rows, id: \.instanceId) { row in
                    VStack(alignment: .leading, spacing: Spacing.s2) {
                        Text(row.name)
                        Text(RestartCopy.reason(row.reason))
                            .font(.callout)
                            .foregroundStyle(StacklyColor.textSecondary)
                    }
                }
            }
        }
    }

    @ViewBuilder private var operations: some View {
        let blocking = preview.blockingOperations ?? []
        if blocking.isEmpty == false {
            VStack(alignment: .leading, spacing: Spacing.s8) {
                Text("Operations in flight").font(.headline)
                ForEach(blocking, id: \.operationId) { operation in
                    VStack(alignment: .leading, spacing: Spacing.s2) {
                        Text(RestartCopy.operation(operation.kind))
                        Text(
                            operation.resumable
                                ? "Carries on after the restart."
                                : "Can't carry on through a restart. stacklyd waits up to 30 seconds for it, then refuses."
                        )
                        .font(.callout)
                        .foregroundStyle(StacklyColor.textSecondary)
                    }
                }
            }
        }
    }
}
