import Foundation
import StacklyClient
import SwiftUI

/// The Environments section of the Overview (spec 0044, *Layar*, AC-23): at most five
/// environments by name, each with its status and "{n} of {m} ready · {file}", then Show All.
/// A click opens the environment.
struct OverviewEnvironments: View {
    /// The most environments the section lists.
    static let limit = 5

    let environments: [EnvironmentSummary]
    let home: String
    let open: @MainActor (UUID) -> Void
    let showAll: @MainActor () -> Void

    var body: some View {
        SectionCard(title: SidebarSection.environments.title) {
            ForEach(environments.prefix(Self.limit)) { environment in
                Button {
                    open(environment.id)
                } label: {
                    EnvironmentRow(environment: environment, home: home)
                        .padding(Spacing.compactRowInsets)
                }
                .buttonStyle(.plain)
            }
            if environments.count > Self.limit {
                Button(action: showAll) {
                    Text("Show All")
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
