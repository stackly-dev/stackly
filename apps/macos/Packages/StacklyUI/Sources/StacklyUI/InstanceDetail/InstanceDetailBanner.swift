import StacklyClient
import SwiftUI

/// The banners on top of the Summary tab (spec 0021, *Tab Summary*): the failure banner of
/// spec 0033 for a failed, recovering, or refused instance (I1 and I7 of spec 0022, and F7),
/// otherwise the problem of the instance by rules I2 to I4, and the failed load of the detail.
/// A blocker that needs a secret carries "Enter Secret…", which opens the sheet of spec 0011
/// for the missing references.
struct InstanceDetailBanner: View {
    /// The two buttons of the "Still stopping." banner (spec 0026, AC-16).
    struct Stopping {
        /// Sends `instance.stop` again, with a new budget.
        let stopAgain: @MainActor () -> Void
        /// Force Stop…, which confirms first.
        let forceStop: StacklyAction
    }

    /// What the failure banner reads and where its buttons go (spec 0033).
    struct Failure {
        /// The operations of the instance.
        let model: InstanceFailureModel
        /// The active and the recent operations of the operation center.
        let center: [OperationSummary]
        /// The name of another instance by ID, for a port it owns.
        let instanceNames: (UUID) -> String?
        /// Shows the Logs tab of this page.
        let showLogs: @MainActor () -> Void
    }

    @Environment(\.actionHost) private var host
    @Environment(\.mutationAvailability) private var gate
    @Environment(\.fixtureNow) private var fixtureNow
    @Environment(\.locale) private var locale
    @Environment(\.timeZone) private var timeZone
    @Environment(\.instanceEditAction) private var editInstance
    @Environment(\.shellNavigator) private var navigator

    let summary: InstanceSummary
    let action: StacklyAction
    let failure: Failure
    let detailError: ErrorBody?
    let retryDetail: @MainActor () -> Void
    let enterSecrets: @MainActor ([SecretNeed]) -> Void
    var stopping: Stopping? = nil

    var body: some View {
        if let stopping {
            // Neutral words: `stopping` without an operation also comes from reconciliation
            // or a stop the daemon lost.
            StateBanner(
                tone: .caution, title: "Still stopping.",
                message: "Stopping hasn’t finished. You can wait, stop again, or force stop.",
                actions: [
                    StateAction(title: "Stop Again", mutates: true, perform: stopping.stopAgain),
                    StateAction(
                        title: "Force Stop…", mutates: true,
                        perform: { [host, gate] in stopping.forceStop.run(host: host, gate: gate) }),
                ], base: .window)
        }
        if summary.lifecycle == .recovering, summary.restart.nextAttemptAt != nil {
            // The countdown of the restart moves every second (AC-12).
            TimelineView(.periodic(from: .now, by: 1)) { context in
                failureOrProblem(now: fixtureNow ?? context.date)
            }
        } else {
            failureOrProblem(now: fixtureNow ?? Date())
        }
        if let detailError {
            StateBanner(
                tone: .caution, title: "Couldn’t load instance details.",
                message: DaemonText.sentence(detailError.message), code: detailError.code,
                actions: [StateAction(title: "Retry", mutates: false, perform: retryDetail)],
                base: .window)
        }
    }

    /// The failure banner when the instance has one, else the banner of rules I2 to I4.
    @ViewBuilder
    private func failureOrProblem(now: Date) -> some View {
        if let content = failure.model.content(
            for: summary, center: failure.center, instanceNames: failure.instanceNames, now: now)
        {
            InstanceFailureBanner(
                content: content, mode: .summary,
                actions: FailureBannerActions(
                    summary: summary, main: action, edit: editInstance, navigator: navigator,
                    showLogs: failure.showLogs))
        } else if let item = AttentionRules.banner(
            for: summary, now: now, locale: locale, timeZone: timeZone)
        {
            problem(item)
        }
    }

    @ViewBuilder
    private func problem(_ item: AttentionItem) -> some View {
        if item.rule == "I4", let needs = Self.needs(of: summary), needs.isEmpty == false {
            StateBanner(
                tone: item.tone, title: "Needs a secret to start",
                message: ([SecretNeed.keychainWaitNote(for: needs)].compactMap { $0 }
                    + needs.map(\.reference)).joined(separator: "\n"),
                actions: [
                    StateAction(
                        title: "Enter Secret…", mutates: true,
                        perform: { enterSecrets(needs) })
                ], base: .window)
        } else {
            StateBanner(
                tone: item.tone, title: item.title, message: item.message, base: .window)
        }
    }

    /// The references a `needsSecret` blocker lists, as the rows of the secret sheet.
    static func needs(of summary: InstanceSummary) -> [SecretNeed]? {
        guard case .needsSecret(let blocker) = summary.blocker else { return nil }
        return (blocker.references ?? []).map { SecretNeed(item: $0) }
    }
}
