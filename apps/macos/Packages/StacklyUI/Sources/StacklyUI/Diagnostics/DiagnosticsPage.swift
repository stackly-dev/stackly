import StacklyClient
import SwiftUI

/// The Diagnostics page: the Connection section, which works while the daemon is down, the
/// Background Agent and Command Line sections, then the Doctor (spec 0034, AC-17). The
/// toolbar holds Run Doctor.
///
/// Nothing here changes data: a finding's button only goes to a page or opens a flow that
/// asks first.
struct DiagnosticsPage: View {
    let diagnostics: ConnectionDiagnostics
    let isReconnecting: Bool
    let reconnect: @MainActor () -> Void
    let refresh: @MainActor () -> Void
    /// The routes this build has; `open_storage_settings` has none yet.
    var routes: DoctorRoutes = .thisBuild

    @Environment(DaemonModel.self) private var daemon: DaemonModel?
    @Environment(\.shellNavigator) private var navigator
    @State private var focus: DiagnosticsFocus?
    @State private var appliedRequest: UUID?

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
                    Button(daemon.doctor.running ? DoctorCopy.running : DoctorCopy.runDoctor) {
                        Task { await daemon.doctor.request(.user) }
                    }
                    .disabled(!isConnected || daemon.doctor.running || daemon.doctor.unsupported)
                }
            }
        }
        .task {
            await daemon?.doctor.request(.appeared)
        }
        .onChange(of: navigator?.diagnosticsRequest?.id, initial: true) {
            applyRequest()
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
        if let doctor = daemon?.doctor {
            Task { await doctor.request(.opened) }
        }
    }

    /// The one place a finding's action goes (AC-18): a page, or the endpoints sheet with the
    /// suggested port filled in, which changes nothing until Apply.
    private func open(_ route: DoctorActionRoute, _ finding: DoctorFinding?) {
        switch route {
        case .showInstance(let id):
            navigator?.openInstance(id)
        case .reviewConfig(let id):
            navigator?.openInstance(id, tab: .configuration)
        case .changePort(let id, let endpoint, let port):
            let source = InstanceEditSource(id: id, name: finding?.instance?.name ?? "")
            navigator?.openInstance(
                id, tab: .configuration, edit: .editPorts(source, prefill: [endpoint: port]))
        case .showOperation(let id):
            navigator?.openOperation(id)
        case .openStorageSettings:
            // No route in this build: `DoctorRoutes.thisBuild` leaves it out.
            break
        }
    }
}
