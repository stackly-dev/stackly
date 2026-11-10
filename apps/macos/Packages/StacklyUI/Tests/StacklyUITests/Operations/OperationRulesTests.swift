import Foundation
import StacklyClient
import StacklyTestSupport
import Testing

@testable import StacklyUI

/// Which buttons an operation offers and what its row says (spec 0035 AC-8 to AC-11):
/// Cancel by availability, Resume and Abandon by the recovery actions, Retry for the newest
/// failed start, stop, restart, or reload whose instance allows it, and the note line of every
/// state.
@MainActor
struct OperationRulesTests {
    private let instanceID = OperationSample.instance(1)

    private func instance(_ lifecycle: InstanceLifecycle, reload: Bool = true) throws
        -> InstanceSummary
    {
        try ContractFixture.instance().testing(revision: 3, lifecycle: lifecycle)
            .moved(to: instanceID, reload: reload)
    }

    // MARK: - Cancel

    @Test(
        .evidence(
            covers: ["REQ-054"],
            expected:
                "Cancel is on only for available, off for past_point_of_no_return, and absent for requested, continuing, none, and an unknown value"
        ))
    func cancelFollowsTheAvailability() {
        let cases: [(CancelAvailability?, OperationState, OperationRules.CancelButton)] = [
            (.available, .running, .enabled),
            (.pastPointOfNoReturn, .running, .disabled),
            (.requested, .cancelRequested, .hidden),
            (.continuing, .cancelRequested, .hidden),
            (.none, .succeeded, .hidden),
            (.unrecognized, .running, .hidden),
            // An older daemon: the fallback rule.
            (nil, .queued, .enabled),
            (nil, .cancelRequested, .hidden),
            (nil, .failed, .hidden),
        ]
        for (cancel, state, expected) in cases {
            let operation = OperationSample.make(state: state, cancel: cancel)
            #expect(
                OperationRules.cancel(for: operation) == expected,
                "\(String(describing: cancel)) \(state)")
        }
    }

    // MARK: - Resume and Abandon

    @Test(
        .evidence(
            covers: ["REQ-054"],
            expected:
                "Resume and Abandon show only for needs_recovery operations whose recovery actions name them"
        ))
    func resumeAndAbandonFollowTheRecoveryActions() {
        func make(_ state: OperationState, _ actions: [RecoveryAction]?) -> OperationSummary {
            OperationSample.make(
                state: state,
                recovery: actions.map { OperationSample.recovery(.resume, actions: $0) })
        }
        let both = make(.needsRecovery, [.resume, .abandon])
        #expect(OperationRules.canResume(both) && OperationRules.canAbandon(both))
        let abandonOnly = make(.needsRecovery, [.abandon])
        #expect(OperationRules.canResume(abandonOnly) == false)
        #expect(OperationRules.canAbandon(abandonOnly))
        let none = make(.needsRecovery, [])
        #expect(OperationRules.canResume(none) == false && OperationRules.canAbandon(none) == false)
        let unknown = make(.needsRecovery, [.unrecognized])
        #expect(OperationRules.canResume(unknown) == false)
        let failed = make(.failed, [.resume, .abandon])
        #expect(
            OperationRules.canResume(failed) == false && OperationRules.canAbandon(failed) == false)
    }

    // MARK: - Retry

    @Test(
        .evidence(
            covers: ["REQ-054"],
            expected:
                "Retry shows for the newest failed start, stop, restart, or reload of an instance that still exists and allows that action, with the verb of its kind"
        ))
    func retryIsTheNewestFailedLifecycleOperationOfAnInstanceThatAllowsIt() throws {
        let failedStart = OperationSample.make(2, kind: .startInstance, state: .failed)
        let failed = try instance(.failed)
        #expect(
            OperationRules.retry(
                for: failedStart, among: [failedStart], instance: failed, local: .none) == .start)

        // A newer lifecycle operation of the same instance takes Retry away.
        let newerStop = OperationSample.make(3, kind: .stopInstance, state: .succeeded)
        #expect(
            OperationRules.retry(
                for: failedStart, among: [newerStop, failedStart], instance: failed, local: .none)
                == nil)
        // A newer operation of another kind, or of another instance, does not.
        let newerRename = OperationSample.make(4, kind: .renameInstance, state: .succeeded)
        let otherInstance = OperationSample.make(
            5, kind: .startInstance, instance: OperationSample.instance(2), state: .succeeded)
        #expect(
            OperationRules.retry(
                for: failedStart, among: [otherInstance, newerRename, failedStart],
                instance: failed, local: .none) == .start)

        // The instance is gone, or its action is not allowed now.
        #expect(
            OperationRules.retry(
                for: failedStart, among: [failedStart], instance: nil, local: .none) == nil)
        #expect(
            OperationRules.retry(
                for: failedStart, among: [failedStart], instance: try instance(.running),
                local: .none) == nil)
        #expect(
            OperationRules.retry(
                for: failedStart, among: [failedStart], instance: failed,
                local: .inFlight(.start)) == nil)
    }

    @Test func eachLifecycleKindRetriesWithItsOwnVerbWhenTheInstanceAllowsIt() throws {
        let running = try instance(.running)
        let cases: [(OperationKind, InstanceSummary, InstanceVerb?)] = [
            (.stopInstance, running, .stop),
            (.restartInstance, running, .restart),
            (.reloadInstance, running, .reload),
            (.reloadInstance, try instance(.running, reload: false), nil),
            (.restartInstance, try instance(.stopped), nil),
            (.forceStopInstance, running, nil),
            (.applyConfig, running, nil),
            (.createInstance, running, nil),
        ]
        for (kind, instance, verb) in cases {
            let operation = OperationSample.make(2, kind: kind, state: .failed)
            #expect(
                OperationRules.retry(
                    for: operation, among: [operation], instance: instance, local: .none) == verb,
                "\(kind)")
        }
        // Only a failed operation has Retry.
        let succeeded = OperationSample.make(2, kind: .stopInstance, state: .succeeded)
        #expect(
            OperationRules.retry(
                for: succeeded, among: [succeeded], instance: running, local: .none) == nil)
    }

    // MARK: - Note line

    @Test(
        .evidence(
            covers: ["REQ-054"],
            expected:
                "the note line says Finishing safely for a continuing cancel, can’t be cancelled past the point of no return, and the recovery hint for needs_recovery"
        ))
    func theNoteLineFollowsTheTable() {
        func note(_ operation: OperationSummary) -> String {
            OperationNote.line(for: operation).text
        }
        #expect(note(OperationSample.make(state: .queued)) == "Queued")
        #expect(
            note(OperationSample.make(state: .queued, waitingOn: OperationSample.id(9)))
                == "Waiting for another operation")
        #expect(note(OperationSample.make(state: .running, stage: "render")) == "Render config")
        #expect(
            note(
                OperationSample.make(
                    kind: .installRuntime, instance: nil, state: .running, stage: "commit",
                    cancel: .pastPointOfNoReturn)) == "Commit · can’t be cancelled")
        #expect(
            note(
                OperationSample.make(
                    kind: .installRuntime, instance: nil, state: .running, stage: "download",
                    progress: .counted(CountedProgress(done: 50, total: 100))))
                == "10% · Download")
        #expect(
            note(
                OperationSample.make(
                    kind: .renameInstance, state: .running, stage: "copy",
                    progress: .counted(CountedProgress(done: 3, total: 4))))
                == "75% · Copy")
        #expect(
            note(OperationSample.make(state: .cancelRequested, stage: "spawn", cancel: .requested))
                == "Cancelling · Launch")
        #expect(
            note(OperationSample.make(state: .cancelRequested, stage: "spawn", cancel: .continuing))
                == "Finishing safely · Launch")
        #expect(
            note(
                OperationSample.make(
                    state: .needsRecovery,
                    recovery: OperationSample.recovery(.resume, actions: [.resume])))
                == "It can continue. Resume or abandon it.")
        #expect(
            note(
                OperationSample.make(
                    state: .needsRecovery,
                    recovery: OperationSample.recovery(.manual, actions: [])))
                == "It needs manual cleanup. Open it for details.")
        let failed = OperationNote.line(
            for: OperationSample.make(
                state: .failed,
                failure: OperationSample.failure(
                    "supervisor.endpoint_in_use", "port 5432 is in use")))
        #expect(failed.text == "Port 5432 is in use. · supervisor.endpoint_in_use")
        #expect(failed.isCritical)
        #expect(note(OperationSample.make(state: .cancelled)) == "Cancelled · undone")
        #expect(note(OperationSample.make(state: .succeeded)) == "Done")
        #expect(
            note(OperationSample.make(state: .succeeded, cancelRequestedAt: OperationSample.base))
                == "Done after cancel request")
    }

    @Test func aPartialResultCountsWhatWasDone() {
        let partial = OperationResult(
            resources: nil,
            partial: [
                StacklyClient.PartialItem(item: "a", resource: nil, outcome: .done, code: nil),
                StacklyClient.PartialItem(item: "b", resource: nil, outcome: .failed, code: "x"),
                StacklyClient.PartialItem(item: "c", resource: nil, outcome: .done, code: nil),
            ])
        #expect(
            OperationNote.line(for: OperationSample.make(state: .succeeded, result: partial)).text
                == "2 of 3 done")
        #expect(
            OperationNote.line(for: OperationSample.make(state: .cancelled, result: partial)).text
                == "Cancelled")
    }

    @Test func theFinishedInstallIsNamedByItsRuntime() {
        let runtime = UUID()
        let install = OperationSummary(
            id: OperationSample.id(3), kind: .installRuntime,
            resource: ResourceRef(kind: .runtime, id: runtime.uuidString.lowercased()),
            state: .succeeded, stage: nil, progress: .indeterminate, waitingOn: nil,
            cancelRequestedAt: nil, pointOfNoReturnAt: nil, failure: nil, recovery: nil,
            result: nil, trigger: OperationTriggerInfo(origin: .user, client: .cli),
            createdAt: OperationSample.base, startedAt: OperationSample.base,
            updatedAt: OperationSample.base, revision: 4, cancel: .none)
        #expect(
            OperationTitle.title(
                for: install, instanceNames: [:],
                runtimes: .init(installed: [runtime: "PostgreSQL 17.6"]))
                == "Install PostgreSQL 17.6")
        #expect(OperationTitle.title(for: install, instanceNames: [:]) == "Install runtime")
    }
}

extension InstanceSummary {
    /// The same instance under another ID, with or without reload.
    fileprivate func moved(to id: UUID, reload: Bool) -> InstanceSummary {
        InstanceSummary(
            id: id, name: name, displayName: displayName, revision: revision,
            software: software, provider: provider, runtimeId: runtimeId,
            runtimeVersion: runtimeVersion, lifecycle: lifecycle, desired: desired,
            readiness: readiness, health: health, statusSeq: statusSeq, process: process,
            lastFailure: lastFailure, restart: restart, restartTotal: restartTotal,
            blocker: blocker, endpoints: endpoints, leftoverProcesses: leftoverProcesses,
            logsUnavailable: logsUnavailable, ownership: ownership, initialized: initialized,
            capabilities: InstanceCapabilities(reload: reload), lastExit: lastExit,
            autostart: autostart)
    }
}
