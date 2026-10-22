import StacklyClient
import SwiftUI

/// The Endpoints card of the Summary tab (spec 0021, *Tab Summary*): one row per endpoint
/// with its address and a Copy button when the address is known, then the Process row and the
/// Readiness row, which are always two separate texts (BR-010).
struct EndpointsCard: View {
    let summary: InstanceSummary

    var body: some View {
        CardSection("Endpoints") {
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
            .accessibilityIdentifier(AccessibilityID.InstanceDetail.process)
            LabeledContent("Readiness") {
                Text(ReadinessText.text(for: summary))
            }
            .accessibilityIdentifier(AccessibilityID.InstanceDetail.readiness)
        }
        .labeledContentStyle(.stackly)
        .accessibilityIdentifier(AccessibilityID.InstanceDetail.card("endpoints"))
    }

    private func endpointRow(_ endpoint: EndpointStatus) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: Spacing.s8) {
            LabeledContent(endpoint.name) {
                Text(DetailFormat.endpointValue(endpoint))
                    .font(.callout.monospaced())
            }
            if let address = DetailFormat.endpointCopyText(endpoint) {
                CopyButton(
                    address, what: "\(endpoint.name) address", style: .icon,
                    announcement: "Address copied.", key: "endpoint.\(endpoint.name)")
            }
        }
    }
}
