import StacklyClient
import SwiftUI

/// The approve sheet of a local provider (spec 0037, *Layar*, AC-11, AC-15): the identity
/// line, the approval statement word for word, the permissions, what changed since the last
/// decision, and the footnote. Cancel is the default button; Approve sends the
/// `packageSha256` of the detail this sheet shows.
struct ApproveProviderSheet: View {
    let model: ProviderDecisionModel

    @Environment(\.feedbackCenter) private var feedbackCenter

    var body: some View {
        if let approval = model.approval {
            let detail = approval.detail
            let summary = detail.summary
            StacklySheet(
                title: ProviderTrustCopy.approveTitle(displayName: summary.displayName),
                subtitle: ProviderTrustCopy.identityLine(
                    provider: summary.provider, providerRevision: summary.providerRevision),
                tile: TileSpec(mark: summary.mark, tile: summary.tile, name: summary.displayName),
                primary: StacklyAction(
                    id: "catalog.approve.confirm", title: ProviderTrustCopy.approveAction,
                    mutates: true,
                    isBusy: approval.isWorking,
                    perform: {
                        Task { await model.confirmApprove(feedback: feedbackCenter) }
                    }),
                cancel: StacklyAction(
                    id: "catalog.approve.cancel", title: "Cancel", mutates: false,
                    perform: { model.cancelApprove() }),
                minWidth: 520,
                cancelIsDefault: true
            ) {
                ApproveProviderContent(approval: approval)
            }
        }
    }
}

/// The body of the approve sheet.
struct ApproveProviderContent: View {
    let approval: ProviderDecisionModel.Approval

    var body: some View {
        let local = approval.detail.local
        VStack(alignment: .leading, spacing: Spacing.s16) {
            if let error = approval.error {
                StateBanner(
                    tone: .critical, title: ProviderTrustCopy.approveFailedTitle,
                    message: [error.message, error.hint].compactMap { $0 }
                        .map(DaemonText.sentence).joined(separator: " "),
                    code: error.code)
            }
            VStack(alignment: .leading, spacing: Spacing.s8) {
                Text(ProviderTrustCopy.approvalStatement)
                    .fixedSize(horizontal: false, vertical: true)
                Text(ProviderTrustCopy.approveTrustNote)
                    .font(.callout)
                    .foregroundStyle(StacklyColor.textSecondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
            if let permissions = local?.permissions {
                CardSection("Permissions") {
                    ProviderPermissionsList(permissions: permissions)
                }
            }
            if let changes = local?.changes, changes.isEmpty == false {
                Text(ProviderTrustCopy.importChanges(changes))
                    .font(.callout)
                    .fixedSize(horizontal: false, vertical: true)
            }
            Text(ProviderTrustCopy.approveFootnote)
                .font(.callout)
                .foregroundStyle(StacklyColor.textSecondary)
                .fixedSize(horizontal: false, vertical: true)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }
}
