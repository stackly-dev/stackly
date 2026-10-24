import StacklyClient
import SwiftUI

/// What the Preflight card of the sheet shows for the checks of the daemon (spec 0020,
/// *Sheet New Instance*, *Aksesibilitas*).
enum PreflightMapping {
    /// The status of a check row: `ok` passed, `caution` warning, `error` failed, and a
    /// status this build does not know unrecognized.
    static func status(of status: PreflightStatus) -> CheckStatus {
        switch status {
        case .ok: .passed
        case .caution: .warning
        case .error: .failed
        case .unrecognized: .unrecognized
        }
    }

    /// The rows of the card.
    /// - Parameters:
    ///   - checks: The checks of the last preflight, in the daemon's order.
    ///   - failedKinds: The kinds of row the failed stage of a create points at; they show
    ///     as failed until the draft changes.
    ///   - secretsAreStored: Whether the `secrets` stage is done, so the secret rows say so.
    static func items(
        _ checks: [PreflightCheck], failedKinds: Set<PreflightCheckKind> = [],
        secretsAreStored: Bool = false
    ) -> [CheckItem] {
        checks.map { check in
            if failedKinds.contains(check.kind) {
                return CheckItem(
                    id: check.id, status: .failed, title: check.title,
                    detail: "This step failed when creating. See the message above.")
            }
            let stored = secretsAreStored && check.kind == .secret && check.status != .error
            return CheckItem(
                id: check.id, status: status(of: check.status), title: check.title,
                detail: stored ? "Stored in Keychain" : check.detail)
        }
    }
}

/// The Preflight card (spec 0020): one row per check with its symbol, title, and detail, a
/// spinner before the first reply, and "Couldn’t run preflight." with Retry when it fails.
struct PreflightSection: View {
    let model: CreateInstanceModel

    var body: some View {
        CardSection("Preflight") {
            if let preflight = model.lastPreflight {
                CheckList(
                    PreflightMapping.items(
                        preflight.checks, failedKinds: model.failedStageKinds,
                        secretsAreStored: model.secretsAreStored),
                    layout: .adaptive)
                if model.preflightFailure != nil {
                    failure
                }
            } else if model.preflightFailure != nil {
                failure
            } else {
                DelayedProgress()
                    .frame(maxWidth: .infinity, minHeight: 34)

            }
        }

    }

    private var failure: some View {
        HStack(spacing: Spacing.s12) {
            Label("Couldn’t run preflight.", systemImage: "exclamationmark.triangle.fill")
                .foregroundStyle(Tone.caution.text)
            Spacer(minLength: Spacing.s12)
            Button("Retry") { model.retryPreflight() }

        }
    }
}
