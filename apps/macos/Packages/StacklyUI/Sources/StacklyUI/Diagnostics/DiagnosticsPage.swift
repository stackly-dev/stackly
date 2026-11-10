import StacklyClient
import SwiftUI

/// The Diagnostics page: the Connection section, which works while the daemon is down, the
/// Background Agent and Command Line sections, then the Doctor and the Diagnostic Bundle
/// (spec 0034, AC-17). The toolbar holds Export Bundle… and Run Doctor.
///
/// Nothing here changes data: a finding's button only goes to a page or opens a flow that
/// asks first, and the bundle is saved by the daemon after the person saw its entries.
struct DiagnosticsPage: View {
    let diagnostics: ConnectionDiagnostics
    let isReconnecting: Bool
    let reconnect: @MainActor () -> Void
    let refresh: @MainActor () -> Void
    /// Where the bundle is saved; the app uses the save panel, a test a fixed file.
    var chooser: any BundleDestinationChoosing = SavePanelBundleDestination()
    /// Shows a saved bundle in Finder.
    var reveal: @MainActor (URL) -> Void = { OpenInPorts.system.opener.reveal($0) }
    /// The routes this build has.
    var routes: DoctorRoutes = .thisBuild

    @Environment(DaemonModel.self) private var daemon: DaemonModel?
    @Environment(\.shellNavigator) private var navigator
    @Environment(\.feedbackCenter) private var feedbackCenter
    @Environment(\.openSettings) private var openSettings
    @State private var focus: DiagnosticsFocus?
    @State private var appliedRequest: UUID?
    @State private var bundle: BundleSession?
    @State private var saved: ExportedBundle?

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: Spacing.sectionGap) {
                ConnectionDiagnosticsSection(
                    diagnostics,
                    isReconnecting: isReconnecting,
                    reconnect: reconnect,
                    refresh: refresh
                )
                BackgroundAgentDiagnosticsSection()
                // provisional: F32 (Settings gets its own Command Line row)
                CommandLineToolSection()
                if let daemon {
                    DoctorSection(
                        doctor: daemon.doctor, isConnected: isConnected, focus: focus,
                        routes: routes, open: open, showAll: { focus = nil })
                    DiagnosticBundleSection(isConnected: isConnected, preview: openBundle)
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, Spacing.pageHorizontal)
            .padding(.vertical, Spacing.s22)
        }
        .pageChrome(
            title: SidebarSection.diagnostics.title,
            subtitle: daemon.map { DoctorPresentation.subtitle($0.doctor.report) }
        )
        .toolbar {
            if let daemon {
                ToolbarItem(placement: .primaryAction) {
                    Button(DoctorCopy.exportBundle, action: openBundle)
                        .disabled(!isConnected)
                }
                ToolbarItem(placement: .primaryAction) {
                    Button(daemon.doctor.running ? DoctorCopy.running : DoctorCopy.runDoctor) {
                        Task { await daemon.doctor.request(.user) }
                    }
                    .disabled(!isConnected || daemon.doctor.running || daemon.doctor.unsupported)
                }
            }
        }
        .task {
            // A run needs a connection; Run Doctor and the caught up trigger start one later.
            guard isConnected else { return }
            await daemon?.doctor.request(.appeared)
        }
        .onChange(of: navigator?.diagnosticsRequest?.id, initial: true) {
            applyRequest()
        }
        .sheet(item: $bundle, onDismiss: showSaved) { session in
            BundleSheet(
                model: session.model, chooser: chooser, exported: { saved = $0 })
        }
    }

    /// Whether the doctor can run and a bundle can be built: a connection is open, also in
    /// recovery, where the doctor reports what it cannot check.
    private var isConnected: Bool {
        daemon?.presentation.hasConnection ?? false
    }

    /// Takes the focus of a new request from ``ShellNavigator/openDiagnostics(focus:)`` and
    /// asks the doctor for a run (AC-20).
    private func applyRequest() {
        guard let request = navigator?.diagnosticsRequest, request.id != appliedRequest else {
            return
        }
        appliedRequest = request.id
        focus = request.focus
        if isConnected, let doctor = daemon?.doctor {
            Task { await doctor.request(.opened) }
        }
    }

    /// The one place a finding's action goes (AC-18).
    private func open(_ route: DoctorActionRoute, _ finding: DoctorFinding?) {
        let openSettings = openSettings
        DoctorRouter(
            navigator: navigator, settingsTab: daemon?.settingsTab,
            openSettings: { openSettings() }
        )
        .open(route, finding: finding)
    }

    private func openBundle() {
        guard let daemon, isConnected else { return }
        bundle = BundleSession(
            model: BundleModel(read: { call throws(ClientError) in
                try await daemon.read(call).get()
            }))
    }

    /// "Saved {file} to {folder}." with Show in Finder, sent by the page once the sheet
    /// closed (spec 0067: a sheet never sends feedback).
    private func showSaved() {
        guard let exported = saved else { return }
        saved = nil
        let file = URL(filePath: exported.path)
        var feedback = Feedback.success(
            DoctorCopy.saved(
                file: file.lastPathComponent,
                folder: file.deletingLastPathComponent().lastPathComponent))
        let reveal = reveal
        feedback.action = StacklyAction(
            id: "bundle.reveal", title: DoctorCopy.showInFinder, mutates: false,
            perform: { reveal(file) })
        feedbackCenter?.show(feedback)
    }
}

/// One opening of the bundle sheet, with its own model.
private struct BundleSession: Identifiable {
    let id = UUID()
    let model: BundleModel
}
