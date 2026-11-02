import SwiftUI

/// The Daemon section of the General tab (spec 0030 AC-16): the stacklyd row with its status
/// line and the Restart Daemon button, and the confirmation sheet the button opens.
struct DaemonSettingsSection: View {
    let model: DaemonModel

    private var restart: DaemonRestartModel { model.restart }

    /// What the row says, from the connection.
    private var content: DaemonSettingsContent {
        DaemonSettingsContent.make(
            isConnected: model.presentation.hasConnection,
            protocolText: model.lastDaemon.map { ShellStatusMapping.text($0.accepted.protocol) },
            launchContext: model.lastDaemon?.info?.launch?.context,
            isRestarting: restart.isRestarting)
    }

    var body: some View {
        let content = content
        Section("Daemon") {
            LabeledContent {
                ActionButton(
                    StacklyAction(
                        id: "settings.daemon.restart",
                        title: RestartCopy.buttonTitle,
                        mutates: true,
                        disabledReason: content.disabledReason,
                        isBusy: restart.isLoadingPreview,
                        perform: { Task { await restart.begin() } }),
                    showsReason: false)
            } label: {
                VStack(alignment: .leading, spacing: Spacing.s2) {
                    Text("stacklyd")
                    Text(content.subtitle)
                        .font(.callout)
                        .foregroundStyle(StacklyColor.textSecondary)
                }
            }
            if let error = restart.previewError {
                StateBanner(
                    tone: .critical, title: DaemonText.sentence(error.message),
                    message: error.hint.map(DaemonText.sentence), code: error.code)
            }
        }
        .sheet(
            isPresented: Binding(
                get: { restart.confirmation != nil },
                set: { if $0 == false { restart.cancel() } })
        ) {
            RestartConfirmationSheet(restart: restart)
        }
    }
}
