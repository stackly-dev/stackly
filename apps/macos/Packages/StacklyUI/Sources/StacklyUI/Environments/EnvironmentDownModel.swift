import Foundation
import Observation
import StacklyClient
import SwiftUI

/// Down of an environment (spec 0047, *Layar*, AC-20 and AC-21), one per Environments page.
/// Down reads `env.preview_down` and opens the confirmation of *Copy Down*; a preview with
/// nothing to stop, no active lease, and no Up to cancel opens none and says "Nothing to bring
/// down." Bring Down sends `env.down` with the IDs of the `stop` list it showed;
/// `env.down_changed` reads the preview again and opens the confirmation with "What Down will
/// stop changed. Review it again." first. When a Down this model sent ends, it reads the
/// operation and shows its result as feedback.
@MainActor
@Observable
final class EnvironmentDownModel {
    /// A Down this model sent, until its operation ends.
    struct Watch: Equatable {
        /// The environment.
        let environment: UUID
        /// Its name, for the feedback.
        let name: String
        /// How many instances the confirmed preview kept running.
        let kept: Int
    }

    /// The environments whose preview or `env.down` is on its way.
    private(set) var busy: Set<UUID> = []
    /// The confirmation that shows, or `nil`.
    var confirmation: ConfirmationRequest?
    /// The Downs this model sent whose operation has not ended, by operation.
    private(set) var watched: [UUID: Watch] = [:]

    /// Shows a feedback card.
    @ObservationIgnored var report: @MainActor (Feedback) -> Void = { _ in }
    /// Opens an operation in Activity.
    @ObservationIgnored var openOperation: @MainActor (UUID) -> Void = { _ in }

    @ObservationIgnored private let store: EnvironmentStore
    @ObservationIgnored private let newKey: () -> String
    @ObservationIgnored private var keys: [UUID: String] = [:]

    /// Creates a model.
    /// - Parameters:
    ///   - store: The environment store, for `env.preview_down`, `env.down`, `operation.get`,
    ///     and the reload after.
    ///   - newKey: Makes an idempotency key.
    init(store: EnvironmentStore, newKey: @escaping () -> String = { MutationPath.newKey() }) {
        self.store = store
        self.newKey = newKey
    }

    /// Whether a preview or an `env.down` of `id` is on its way.
    func isBusy(_ id: UUID) -> Bool {
        busy.contains(id)
    }

    // MARK: - Down

    /// Down: reads the preview of `id`, then opens the confirmation, or says there is nothing
    /// to bring down.
    func begin(environment id: UUID) async {
        guard busy.contains(id) == false else { return }
        busy.insert(id)
        defer { busy.remove(id) }
        let preview: EnvDownPreview
        do throws(ClientError) {
            preview = try await store.previewDown(id)
        } catch {
            report(.failure(error.body))
            return
        }
        guard Self.hasWork(preview) else {
            report(.neutral(EnvironmentCopy.nothingToBringDown))
            return
        }
        present(preview, notice: nil)
    }

    /// Whether Down has anything to do for `preview`: an instance to stop, an active lease to
    /// release, or an Up to cancel.
    static func hasWork(_ preview: EnvDownPreview) -> Bool {
        preview.stop.isEmpty == false || preview.leaseCount > 0 || preview.cancelsUp
    }

    /// The confirmation of `preview`, with `notice` as its first line.
    func request(_ preview: EnvDownPreview, notice: String?) -> ConfirmationRequest {
        ConfirmationRequest(
            title: EnvironmentCopy.bringDownTitle(preview.environment.name),
            message: EnvironmentCopy.downMessage(preview, notice: notice),
            actionTitle: EnvironmentCopy.bringDown, role: .standard,
            perform: { [weak self] in
                Task { await self?.bringDown(preview) }
            })
    }

    private func present(_ preview: EnvDownPreview, notice: String?) {
        confirmation = request(preview, notice: notice)
    }

    /// Bring Down: `env.down` with the `stop` IDs of `preview`. An accepted Down is watched
    /// until its operation ends; `env.down_changed` opens the confirmation again with the new
    /// preview; any other refusal shows as feedback. A retry after an unknown outcome reuses
    /// the key.
    func bringDown(_ preview: EnvDownPreview) async {
        let id = preview.environment.id
        guard busy.contains(id) == false else { return }
        busy.insert(id)
        defer { busy.remove(id) }
        let key = keys[id] ?? newKey()
        keys[id] = key
        let outcome = await store.send(
            .envDown(
                EnvDownParams(id: id, expectedStopInstanceIds: preview.stop.map(\.instanceId))),
            key)
        if case .outcomeUnknown = outcome {} else { keys[id] = nil }
        switch outcome {
        case .completed(.envDown(let reply)):
            let operation = reply.operation.operation
            let watch = Watch(environment: id, name: reply.name, kept: preview.keep.count)
            await store.reload()
            if EnvironmentStore.hasEnded(operation.state) {
                show(operation, watch: watch)
            } else {
                watched[operation.id] = watch
            }
        case .completed:
            report(.failure(ClientErrorCatalog.protocolViolation()))
        case .rejected(let body) where body.code == "env.down_changed":
            do throws(ClientError) {
                let fresh = try await store.previewDown(id)
                present(fresh, notice: EnvironmentCopy.downChanged)
            } catch {
                report(.failure(error.body))
            }
        default:
            if let feedback = Feedback.outcome(outcome, success: nil, showOperations: nil) {
                report(feedback)
            }
        }
    }

    // MARK: - Result

    /// An environment operation ended: when it is a Down this model sent, reads it and shows
    /// its result.
    func ended(_ ended: EndedOperation?) async {
        guard let ended, let watch = watched.removeValue(forKey: ended.operation) else { return }
        do throws(ClientError) {
            show(try await store.operation(ended.operation), watch: watch)
        } catch {
            return
        }
    }

    private func show(_ operation: OperationSummary, watch: Watch) {
        let environment = watch.environment
        let feedback = Self.feedback(
            operation, name: watch.name, kept: watch.kept,
            bringDownAgain: { [weak self] in
                Task { await self?.begin(environment: environment) }
            },
            showOperation: { [weak self] in self?.openOperation(operation.id) })
        if let feedback { report(feedback) }
    }

    /// The feedback of a Down that ended (spec 0047, *Copy Down*): `{s}` from
    /// `result.resources`, `{k}` from the kept instances of the confirmed preview plus the
    /// `partial` items with `env.instance_in_use`, `{f}` from `details.count`.
    /// - Parameters:
    ///   - operation: The summary of the `environment_down` that ended.
    ///   - name: The environment's name.
    ///   - kept: How many instances the confirmed preview kept running.
    ///   - bringDownAgain: Opens Down again, for a Down that left instances running.
    ///   - showOperation: Opens the operation.
    static func feedback(
        _ operation: OperationSummary, name: String, kept: Int,
        bringDownAgain: @escaping @MainActor () -> Void,
        showOperation: @escaping @MainActor () -> Void
    ) -> Feedback? {
        switch operation.state {
        case .succeeded:
            let result = operation.result
            let stopped = (result?.resources ?? []).count { $0.kind == .instance }
            let inUse = (result?.partial ?? []).count { $0.code == "env.instance_in_use" }
            let keptCount = kept + inUse
            if keptCount > 0 {
                // The environment can read partially ready afterwards: a shared or manually
                // started instance it uses still runs. The detail says so with the sentence of
                // the confirmation.
                return .success(
                    EnvironmentCopy.downKept(name, stopped: stopped, kept: keptCount),
                    detail: EnvironmentCopy.downKeptDetail)
            }
            if stopped > 0 {
                return .success(EnvironmentCopy.downStopped(name, stopped: stopped))
            }
            return .success(EnvironmentCopy.downReleased(name))
        case .failed:
            guard let error = operation.failure?.error else { return nil }
            guard error.code == "env.down_incomplete" else {
                var feedback = Feedback.failure(error)
                feedback.action = showAction(showOperation)
                return feedback
            }
            let items = (error.items ?? []).filter { $0.kind == "instance" }
            let names = items.compactMap { field("name", of: $0) }
            let leftRunning =
                items.isEmpty == false
                && items.allSatisfy { field("outcome", of: $0) == "left_running" }
            if leftRunning {
                return Feedback(
                    tone: .caution,
                    message: EnvironmentCopy.downLeftRunning(name, instances: names),
                    code: error.code,
                    action: StacklyAction(
                        id: "environment.down.again", title: EnvironmentCopy.bringDownAgain,
                        mutates: true, perform: bringDownAgain))
            }
            let count =
                detail("count", of: error).flatMap(Int.init) ?? max(items.count, names.count)
            return Feedback(
                tone: .caution,
                message: EnvironmentCopy.downIncomplete(name, instances: names, count: count),
                code: error.code, action: showAction(showOperation))
        case .cancelled:
            return .neutral(
                operation.stage == "stop"
                    ? EnvironmentCopy.downCancelledAfterRelease(name)
                    : EnvironmentCopy.downCancelledBeforeStart(name))
        case .needsRecovery:
            return Feedback(
                tone: .caution, message: OperationNote.recovery(operation.recovery?.hint),
                action: showAction(showOperation))
        case .queued, .running, .cancelRequested:
            return nil
        }
    }

    private static func showAction(_ perform: @escaping @MainActor () -> Void) -> StacklyAction {
        StacklyAction(
            id: "environment.down.showOperation", title: EnvironmentCopy.showOperation,
            mutates: false, perform: perform)
    }

    private static func field(_ key: String, of item: ErrorItem) -> String? {
        item.fields?.first { $0.key == key }?.value
    }

    private static func detail(_ key: String, of error: ErrorBody) -> String? {
        error.details?.first { $0.key == key }?.value
    }
}

extension EnvironmentValues {
    /// Down of the Environments page, or `nil` outside it (a preview or a snapshot of a page
    /// alone): then the toolbar shows no Down.
    @Entry var environmentDown: EnvironmentDownModel? = nil
}
