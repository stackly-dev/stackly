#if DEBUG
    import Foundation
    import StacklyClient
    import SwiftUI

    /// A create service the caller scripts: each method runs the closure of the same name,
    /// and every call is kept (spec 0020: previews, snapshots, and model tests).
    @MainActor
    final class FixtureCreateService: CreateInstanceService {
        /// The calls made, in order.
        private(set) var formCalls: [UUID] = []
        private(set) var duplicateCalls: [UUID] = []
        private(set) var preflightCalls: [PreflightCreateParams] = []
        private(set) var listCalls = 0

        var onForm: (UUID) async throws(ClientError) -> CreateForm = { _ in
            FixtureCreateForm.database
        }
        var onDuplicate: (UUID) async throws(ClientError) -> DuplicateForm = { _ in
            FixtureCreateDuplicate.pg17
        }
        var onPreflight: (PreflightCreateParams) async throws(ClientError) -> CreatePreflight = {
            _ in FixtureCreateForm.ready
        }
        var onSecrets: () async throws(ClientError) -> [SecretRefSummary] = {
            FixtureCreateForm.secrets
        }

        func getCreateForm(runtimeId: UUID, provider: String?) async throws(ClientError)
            -> CreateForm
        {
            formCalls.append(runtimeId)
            return try await onForm(runtimeId)
        }

        func getDuplicateForm(instanceId: UUID) async throws(ClientError) -> DuplicateForm {
            duplicateCalls.append(instanceId)
            return try await onDuplicate(instanceId)
        }

        func preflight(_ draft: PreflightCreateParams) async throws(ClientError)
            -> CreatePreflight
        {
            preflightCalls.append(draft)
            return try await onPreflight(draft)
        }

        func listSecrets() async throws(ClientError) -> [SecretRefSummary] {
            listCalls += 1
            return try await onSecrets()
        }
    }

    /// The stores and the model the New Instance sheet needs, over fixture data and with
    /// answers the caller controls.
    @MainActor
    final class CreateRig {
        /// What the model sent, announced, and reported, and what it waits on.
        final class Sink {
            var sent: [(call: Call, key: String)] = []
            var announced: [String] = []
            var finished: [CreateOutcome] = []
            var watchers: [UUID: AsyncStream<OperationSnapshot>.Continuation] = [:]
            /// What `send` answers for a call; the default accepts a create and a start.
            var answer: (Call, String) -> MutationOutcome = { call, _ in
                switch call {
                case .instanceCreate:
                    .completed(
                        .instanceCreate(
                            InstanceActionAccepted(
                                accepted: OperationAccepted(
                                    operation: CreateRig.operation(
                                        state: .running, stage: "validate"),
                                    replayed: false),
                                alreadyInState: false)))
                case .instanceStart:
                    .completed(
                        .instanceStart(
                            InstanceActionAccepted(accepted: nil, alreadyInState: false)))
                default:
                    .notSent(ClientErrorCatalog.notConnected())
                }
            }
        }

        let service = FixtureCreateService()
        let runtimeRig: RuntimeRig
        let pageRig: PageRig
        let model: CreateInstanceModel
        private let sink = Sink()

        /// Every mutation the model sent, with its key.
        var sent: [(call: Call, key: String)] { sink.sent }
        /// What the model announced.
        var announced: [String] { sink.announced }
        /// What the model reported when a create succeeded.
        var finished: [CreateOutcome] { sink.finished }
        /// What `send` answers for a call.
        var answer: (Call, String) -> MutationOutcome {
            get { sink.answer }
            set { sink.answer = newValue }
        }

        /// Builds the rig; nothing is loaded until ``load()``.
        init(runtimes page: RuntimeListPage = FixtureRuntime.page) {
            let runtimeRig = RuntimeRig(page: page)
            let pageRig = PageRig()
            let sink = sink
            self.runtimeRig = runtimeRig
            self.pageRig = pageRig
            model = CreateInstanceModel(
                runtimes: runtimeRig.runtimes, catalog: runtimeRig.catalog,
                instances: pageRig.instances, service: service,
                send: { call, key in
                    sink.sent.append((call, key))
                    return sink.answer(call, key)
                },
                updates: { id in
                    let (stream, continuation) = AsyncStream.makeStream(of: OperationSnapshot.self)
                    sink.watchers[id] = continuation
                    return stream
                },
                debounce: .zero, sleep: { _ in },
                announce: { sink.announced.append($0) },
                onFinish: { sink.finished.append($0) })
        }

        /// Loads the stores once, as the first snapshot round of the shell does.
        func load() async {
            await runtimeRig.load()
            await pageRig.loadInstances()
        }

        /// Gives the model an update of the operation; a final state ends the stream.
        func emit(_ operation: OperationSummary) {
            sink.watchers[operation.id]?.yield(operation)
            switch operation.state {
            case .succeeded, .failed, .cancelled, .needsRecovery:
                sink.watchers[operation.id]?.finish()
            default: break
            }
        }

        /// The ID of every create operation of the rig.
        static let operationID = FixtureID.uuid(kind: FixtureID.operation, number: 9)

        /// The create operation in `state`, at `stage`.
        static func operation(
            state: OperationState, stage: String?, failure: OperationFailureInfo? = nil,
            instance: UUID? = nil, revision: UInt64 = 1
        ) -> OperationSummary {
            let at = FixtureClock.minutesAgo(1)
            return OperationSummary(
                id: operationID, kind: .createInstance, resource: nil, state: state, stage: stage,
                progress: .indeterminate, waitingOn: nil, cancelRequestedAt: nil,
                pointOfNoReturnAt: nil, failure: failure, recovery: nil,
                result: instance.map {
                    OperationResult(
                        resources: [ResourceRef(kind: .instance, id: $0.uuidString.lowercased())],
                        partial: nil)
                }, trigger: OperationTriggerInfo(origin: .user, client: .gui), createdAt: at,
                startedAt: at, updatedAt: at, revision: revision)
        }
    }
#endif
