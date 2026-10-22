import Foundation
import StacklyClient

extension RuntimeStore {
    /// The stage after which a cancel no longer stops an install: the point of no return of
    /// `install_runtime` (spec 0018, *Alur install*).
    static let commitStage = "commit"

    /// Cancel Install, in the context menu of an Installing row (spec 0018, *Halaman
    /// Runtimes*): `operation.cancel` with no confirmation, since nothing the install made
    /// is kept (spec 0012). It is disabled during Commit, the step that cannot be undone.
    ///
    /// - Parameters:
    ///   - install: The install.
    ///   - stage: The stage the operation is in now, or `nil` when unknown.
    ///   - context: Where it reports.
    public func cancelAction(
        for install: InstallingRuntime, stage: String?, context: InstanceActionContext
    ) -> StacklyAction {
        StacklyAction(
            id: "cancelInstall", title: "Cancel Install", mutates: true,
            disabledReason: stage == Self.commitStage ? "Can’t be cancelled during Commit" : nil,
            perform: { [self] in
                Task { await cancel(install, context: context) }
            })
    }

    /// Sends `operation.cancel` for `install` and reports what the daemon did.
    func cancel(_ install: InstallingRuntime, context: InstanceActionContext) async {
        let outcome = await send(
            .operationCancel(CancelOperationParams(operationId: install.operationId)),
            MutationPath.newKey())
        var success: Feedback?
        if case .completed(.operationCancel(let reply)) = outcome,
            reply.disposition == .continuingPastPointOfNoReturn
        {
            success = .neutral(
                "\(install.displayName) \(install.version) can’t be cancelled any more. It will finish installing."
            )
        }
        if let feedback = Feedback.outcome(
            outcome, success: success, showOperations: context.showOperations)
        {
            context.report(feedback)
        }
    }
}
