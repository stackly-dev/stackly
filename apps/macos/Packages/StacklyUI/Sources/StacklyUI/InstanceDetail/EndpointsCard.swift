import StacklyClient
import SwiftUI

/// The Endpoints card of the Summary tab (spec 0021, *Tab Summary*): one row per endpoint
/// with its address and a Copy button when the address is known, then the Process row and the
/// Readiness row, which are always two separate texts (BR-010).
struct EndpointsCard: View {
    @Environment(\.instanceEditAction) private var editInstance

    let summary: InstanceSummary

    /// The Edit button of the card: Edit Ports… under its short name (spec 0029, AC-19).
    private var edit: StacklyAction? {
        guard
            var action = editInstance?.editPortsAction(
                for: InstanceEditSource(id: summary.id, name: summary.name))
        else { return nil }
        action.title = "Edit"
        return action
    }

    var body: some View {
        CardSection(
            "Endpoints",
            accessory: {
                if let edit {
                    ActionButton(edit, showsReason: false)
                        .controlSize(.small)
                }
            },
            footer: { EmptyView() }
        ) {
            let endpoints = summary.endpoints ?? []
            if endpoints.isEmpty {
                Text("This instance has no endpoints.")
                    .foregroundStyle(StacklyColor.textSecondary)
            }
            ForEach(Array(endpoints.enumerated()), id: \.offset) { _, endpoint in
                endpointRow(endpoint)
            }
            LabeledContent("Process") {
                Text(ProcessText.text(for: summary))
            }

            LabeledContent("Readiness") {
                Text(ReadinessText.text(for: summary))
            }

        }
        .labeledContentStyle(.stackly)

    }

    private func endpointRow(_ endpoint: EndpointStatus) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: Spacing.s8) {
            LabeledContent(endpoint.name) {
                Text(DetailFormat.endpointValue(endpoint))
                    .font(.callout.monospaced())
            }
            if let address = DetailFormat.endpointCopyText(endpoint) {
                CopyButton(
                    address, what: "\(endpoint.name) address", style: .icon)
            }
        }
    }
}
