import StacklyClient
import SwiftUI

/// The banners on top of the Summary tab (spec 0021, *Tab Summary*): the problem of the
/// instance by rules I1 to I4 of spec 0022, and the failed load of the detail.
/// A failed instance carries the output of its last process and Retry; a blocker that needs a
/// secret carries "Enter Secret…", which opens the sheet of spec 0011 for the missing
/// references.
struct InstanceDetailBanner: View {
    @Environment(\.fixtureNow) private var fixtureNow
    @Environment(\.locale) private var locale
    @Environment(\.timeZone) private var timeZone

    let summary: InstanceSummary
    let action: StacklyAction
    let detailError: ErrorBody?
    let retryDetail: @MainActor () -> Void
    let enterSecrets: @MainActor ([SecretNeed]) -> Void

    var body: some View {
        if let item = AttentionRules.banner(
            for: summary, now: fixtureNow ?? Date(), locale: locale, timeZone: timeZone)
        {
            problem(item)
        }
        if let detailError {
            StateBanner(
                tone: .caution, title: "Couldn’t load instance details.",
                message: DaemonText.sentence(detailError.message), code: detailError.code,
                actions: [StateAction(title: "Retry", mutates: false, perform: retryDetail)],
                base: .window)
        }
    }

    @ViewBuilder
    private func problem(_ item: AttentionItem) -> some View {
        if item.rule == "I4", let needs = Self.needs(of: summary), needs.isEmpty == false {
            StateBanner(
                tone: item.tone, title: "Needs a secret to start",
                message:
                    needs.map(\.reference).joined(separator: "\n"),
                actions: [
                    StateAction(
                        title: "Enter Secret…", mutates: true,
                        perform: { enterSecrets(needs) })
                ], base: .window)
        } else {
            StateBanner(
                tone: item.tone, title: item.title, message: item.message,
                code: item.rule == "I1" ? InstanceInspector.excerpt(summary.lastFailure) : nil,
                actions: retry(for: item), base: .window)
        }
    }

    /// Retry on a failed instance, when the main action is Retry and it can run.
    private func retry(for item: AttentionItem) -> [StateAction] {
        guard item.rule == "I1", action.id == InstanceAction.Kind.retry.rawValue,
            action.disabledReason == nil
        else { return [] }
        return [
            StateAction(title: "Retry", mutates: true, perform: { action.perform() })
        ]
    }

    /// The references a `needsSecret` blocker lists, as the rows of the secret sheet.
    static func needs(of summary: InstanceSummary) -> [SecretNeed]? {
        guard case .needsSecret(let blocker) = summary.blocker else { return nil }
        return (blocker.references ?? []).map { SecretNeed(item: $0) }
    }
}
