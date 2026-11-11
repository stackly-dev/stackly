import Foundation
import Observation
import StacklyClient

/// Approve, revoke, and remove for one local provider (spec 0037, *Layar*, AC-15).
/// Approve opens a sheet and sends `provider.approve` with the `packageSha256` of the
/// detail the sheet shows, so an approval never covers bytes the person did not see.
/// Revoke and remove ask through the native confirmation dialog first. The daemon decides
/// whether each is allowed; the model only sends and says what came back.
@MainActor
@Observable
final class ProviderDecisionModel {
    /// The open approve sheet.
    struct Approval: Equatable {
        /// The detail the sheet shows.
        let detail: ProviderDetail
        /// Whether `provider.approve` is on its way.
        var isWorking = false
        /// Why the last approval did not go through, or `nil`.
        var error: ErrorBody?
    }

    /// The open approve sheet, or `nil`.
    var approval: Approval?
    /// The confirmation the page shows, or `nil`.
    var confirmation: ConfirmationRequest?
    /// The providers with a revoke or remove on its way.
    private(set) var working: Set<String> = []

    @ObservationIgnored private let send: (Call, String) async -> MutationOutcome
    @ObservationIgnored private let refresh: @MainActor () async -> Void
    @ObservationIgnored private let newKey: () -> String
    @ObservationIgnored private var approvalKey: String?

    /// Creates a model.
    /// - Parameters:
    ///   - send: Sends a mutation with its idempotency key.
    ///   - refresh: Loads the catalog again after a change.
    ///   - newKey: Makes an idempotency key.
    init(
        send: @escaping (Call, String) async -> MutationOutcome,
        refresh: @escaping @MainActor () async -> Void,
        newKey: @escaping () -> String = { MutationPath.newKey() }
    ) {
        self.send = send
        self.refresh = refresh
        self.newKey = newKey
    }

    // MARK: - Approve

    /// Opens the approve sheet on `detail`.
    func beginApprove(_ detail: ProviderDetail) {
        approvalKey = nil
        approval = Approval(detail: detail)
    }

    /// Closes the approve sheet without sending anything (spec 0037 AC-15: Cancel is always
    /// on). A `provider.approve` on its way still runs in the daemon; its answer is dropped,
    /// and the catalog refresh shows the real state.
    func cancelApprove() {
        approvalKey = nil
        approval = nil
    }

    /// Sends `provider.approve` with the digest of the detail the sheet shows.
    /// `provider.import_changed` closes the sheet and loads the detail again, since the
    /// package the person saw is gone; any other refusal stays in the sheet.
    /// - Parameter feedback: Where the result is said, or `nil`.
    func confirmApprove(feedback: FeedbackCenter?) async {
        guard var current = approval, current.isWorking == false,
            let digest = current.detail.local?.packageSha256
        else { return }
        let detail = current.detail
        let key = approvalKey ?? newKey()
        approvalKey = key
        current.isWorking = true
        current.error = nil
        approval = current
        let outcome = await send(
            .providerApprove(
                DecideProviderParams(
                    provider: detail.summary.provider, expectedPackageSha256: digest)), key)
        // Cancel may have closed the sheet, or another one opened: the answer then says
        // nothing and writes nothing, and only the refresh shows what the daemon did.
        guard approval?.detail == detail, approval?.isWorking == true else {
            await refresh()
            return
        }
        switch outcome {
        case .completed:
            approvalKey = nil
            approval = nil
            feedback?.show(
                .success(ProviderTrustCopy.approved(displayName: detail.summary.displayName)))
            await refresh()
        case .rejected(let body) where body.code == "provider.import_changed":
            approvalKey = nil
            approval = nil
            feedback?.show(.failure(body))
            await refresh()
        case .rejected(let body), .notSent(let body):
            approvalKey = nil
            fail(body)
        case .outcomeUnknown(let body, _):
            fail(body)
        }
    }

    private func fail(_ body: ErrorBody) {
        guard var current = approval else { return }
        current.isWorking = false
        current.error = body
        approval = current
    }

    // MARK: - Revoke and remove

    /// Asks to revoke the approval of `detail`.
    func requestRevoke(_ detail: ProviderDetail, feedback: FeedbackCenter?) {
        let name = detail.summary.displayName
        confirmation = ConfirmationRequest(
            title: ProviderTrustCopy.revokeTitle(displayName: name),
            message: ProviderTrustCopy.revokeMessage,
            actionTitle: ProviderTrustCopy.revokeAction,
            role: .destructive,
            perform: { [weak self] in
                Task { await self?.revoke(detail, feedback: feedback) }
            })
    }

    /// Asks to remove `detail` from the catalog.
    func requestRemove(_ detail: ProviderDetail, feedback: FeedbackCenter?) {
        let name = detail.summary.displayName
        confirmation = ConfirmationRequest(
            title: ProviderTrustCopy.removeTitle(displayName: name),
            message: ProviderTrustCopy.removeMessage,
            actionTitle: ProviderTrustCopy.removeAction,
            role: .destructive,
            perform: { [weak self] in
                Task { await self?.remove(detail, feedback: feedback) }
            })
    }

    /// Sends `provider.revoke`.
    func revoke(_ detail: ProviderDetail, feedback: FeedbackCenter?) async {
        let provider = detail.summary.provider
        await run(
            .providerRevoke(ProviderTargetParams(provider: provider)), provider: provider,
            success: ProviderTrustCopy.revoked(displayName: detail.summary.displayName),
            feedback: feedback)
    }

    /// Sends `provider.remove`.
    func remove(_ detail: ProviderDetail, feedback: FeedbackCenter?) async {
        let provider = detail.summary.provider
        await run(
            .providerRemove(ProviderTargetParams(provider: provider)), provider: provider,
            success: ProviderTrustCopy.removed(displayName: detail.summary.displayName),
            feedback: feedback)
    }

    private func run(
        _ call: Call, provider: String, success: String, feedback: FeedbackCenter?
    ) async {
        guard working.insert(provider).inserted else { return }
        defer { working.remove(provider) }
        let outcome = await send(call, newKey())
        if let shown = Feedback.outcome(
            outcome, success: .success(success), showOperations: nil)
        {
            feedback?.show(shown)
        }
        if case .completed = outcome {
            await refresh()
        }
    }
}
