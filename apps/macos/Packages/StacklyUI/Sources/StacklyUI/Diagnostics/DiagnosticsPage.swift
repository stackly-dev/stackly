import SwiftUI

/// The Diagnostics page: the Connection section, which works while the daemon is down,
/// then the place where feature 34 adds the doctor results.
struct DiagnosticsPage: View {
    let diagnostics: ConnectionDiagnostics
    let isReconnecting: Bool
    let reconnect: @MainActor () -> Void
    let refresh: @MainActor () -> Void

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
                // provisional: F34 (the doctor results go below the Connection section)
                UnbuiltPage(section: .diagnostics)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, Spacing.pageHorizontal)
            .padding(.vertical, Spacing.s22)
        }
        .pageChrome(title: SidebarSection.diagnostics.title)
    }
}
