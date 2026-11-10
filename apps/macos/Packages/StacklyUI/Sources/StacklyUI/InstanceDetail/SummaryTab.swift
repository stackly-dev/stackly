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
    /// What the failure banner of spec 0033 reads and where its buttons go.
    let failure: InstanceDetailBanner.Failure
    /// The actions of the "Still stopping." banner, or `nil` when it does not show.
    var stopping: InstanceDetailBanner.Stopping? = nil

    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.sectionGap) {
            InstanceDetailBanner(
                summary: summary, action: action, failure: failure,
                detailError: model.detailError,
                retryDetail: { Task { await model.reload() } }, enterSecrets: enterSecrets,
                stopping: stopping)
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
        .task(id: summary.id) { await failure.model.open() }
        .onChange(of: summary, initial: true) { _, new in
            failure.model.observe(new)
        }
        .onChange(of: failure.center, initial: true) { _, new in
            failure.model.observe(center: new)
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
