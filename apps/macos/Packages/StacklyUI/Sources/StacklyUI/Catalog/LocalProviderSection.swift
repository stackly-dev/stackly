import StacklyClient
import SwiftUI

/// The approval banner of a local provider, under the header of the detail panel (spec
/// 0037, *Banner approval*): "Not approved yet", "Changed since your approval", "Approval
/// revoked", or "Revoked by the catalog", with Approve… or Approve Again… when the person
/// can approve. An approved entry has no banner.
struct LocalProviderBanner: View {
    let detail: ProviderDetail
    let catalog: CatalogInfo?
    let approve: @MainActor () -> Void

    @Environment(\.locale) private var locale
    @Environment(\.calendar) private var calendar
    @Environment(\.timeZone) private var timeZone

    var body: some View {
        if let banner = ProviderTrustCopy.banner(
            detail: detail, catalog: catalog,
            style: LocalProviderSection.dateStyle(
                locale: locale, calendar: calendar, timeZone: timeZone))
        {
            StateBanner(
                tone: banner.tone, title: banner.title, message: banner.message,
                actions: banner.action.map { action in
                    [
                        StateAction(
                            title: action == .approve
                                ? ProviderTrustCopy.approveButton
                                : ProviderTrustCopy.approveAgainButton,
                            mutates: true, perform: approve)
                    ]
                } ?? [])
        }
    }
}

/// The part of the detail panel only a local provider has (spec 0037, *Layar*): the
/// Permissions section and the Revoke Approval… and Remove… buttons. Revoke shows while
/// the entry has an approval to revoke: it is approved, or it waits for approval of new
/// bytes after the person approved older ones.
struct LocalProviderSection: View {
    let detail: ProviderDetail
    let isWorking: Bool
    let revoke: @MainActor () -> Void
    let remove: @MainActor () -> Void

    /// How the panel writes a date: "Nov 11, 2026".
    static func dateStyle(locale: Locale, calendar: Calendar, timeZone: TimeZone)
        -> Date.FormatStyle
    {
        Date.FormatStyle(
            date: .abbreviated, time: .omitted, locale: locale, calendar: calendar,
            timeZone: timeZone)
    }

    /// Whether the entry has an approval the person can revoke.
    static func canRevoke(_ detail: ProviderDetail) -> Bool {
        if detail.summary.approval == .approved { return true }
        return detail.summary.approval == .pending && detail.local?.decision?.decision == .approved
    }

    var body: some View {
        if let permissions = detail.local?.permissions {
            CardSection("Permissions") {
                ProviderPermissionsList(permissions: permissions)
            }
        }
        HStack(spacing: Spacing.s8) {
            if Self.canRevoke(detail) {
                ActionButton(
                    StacklyAction(
                        id: "catalog.provider.revoke", title: ProviderTrustCopy.revokeButton,
                        mutates: true, isBusy: isWorking, perform: revoke))
            }
            ActionButton(
                StacklyAction(
                    id: "catalog.provider.remove", title: ProviderTrustCopy.removeButton,
                    mutates: true, isBusy: isWorking, perform: remove))
        }
    }
}
