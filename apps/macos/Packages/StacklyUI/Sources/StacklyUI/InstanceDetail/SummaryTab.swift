import StacklyClient
import SwiftUI

/// The Summary tab (spec 0021, *Tab Summary*, PAGE-05): the banners, the reason the main
/// action is off, and four cards. From 760 pt of content they stand in two columns (Endpoints,
/// Connection, and Dependents on the left; Details on the right), below that in one column in
/// the same order.
struct SummaryTab: View {
    /// The content width from which the cards stand in two columns.
    static let twoColumnMinWidth: CGFloat = 760

    @State private var width: CGFloat = 0

    let summary: InstanceSummary
    let action: StacklyAction
    let model: InstanceDetailModel
    let secrets: SecretsClient
    let home: String
    let enterSecrets: @MainActor ([SecretNeed]) -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.sectionGap) {
            InstanceDetailBanner(
                summary: summary, action: action, detailError: model.detailError,
                retryDetail: { Task { await model.reload() } }, enterSecrets: enterSecrets)
            if let reason = action.disabledReason {
                DisabledReasonText(reason)
            }
            cards
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .onGeometryChange(for: CGFloat.self) {
            $0.size.width
        } action: {
            width = $0
        }
    }

    @ViewBuilder private var cards: some View {
        if width >= Self.twoColumnMinWidth {
            HStack(alignment: .top, spacing: Spacing.sectionGap) {
                VStack(alignment: .leading, spacing: Spacing.sectionGap) {
                    endpoints
                    connection
                    dependents
                }
                .frame(maxWidth: .infinity, alignment: .top)
                details
                    .frame(minWidth: 280, idealWidth: 320, maxWidth: 360, alignment: .top)
            }
        } else {
            VStack(alignment: .leading, spacing: Spacing.sectionGap) {
                endpoints
                connection
                dependents
                details
            }
        }
    }

    private var endpoints: some View { EndpointsCard(summary: summary) }

    private var connection: some View {
        ConnectionCard(summary: summary, model: model, secrets: secrets)
    }

    private var dependents: some View {
        DependentsCard(detail: model.detail, isPending: model.isDetailPending)
    }

    private var details: some View { DetailsCard(summary: summary, model: model, home: home) }
}
