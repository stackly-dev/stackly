import Foundation
import StacklyClient
import StacklyTestSupport
import Testing

@testable import StacklyUI

/// The stage definitions of spec 0035 (*Definisi tahap*, AC-14): every stage key a supervisor
/// job enters maps to the index of its stage, an alias keeps the bar on that stage and shows
/// its own title, and a key no definition knows takes the fallback label.
struct OperationStagesTests {
    /// One kind with every key its job enters, in the order the job enters them, and the
    /// stage index and text each one shows.
    nonisolated struct Mapping: CustomTestStringConvertible, Sendable {
        let kind: OperationKind
        let keys: [(key: String, index: Int, title: String)]

        var testDescription: String { kind.rawValue }
    }

    nonisolated static let mappings: [Mapping] = [
        Mapping(
            kind: .startInstance,
            keys: [
                ("validate", 0, "Check"), ("lock", 0, "Check"), ("verify", 0, "Check"),
                ("endpoints", 0, "Check"), ("journal", 0, "Check"),
                ("render", 1, "Render config"), ("spawn", 2, "Launch"),
                ("identity", 2, "Launch"), ("backoff", 2, "Waiting to restart"),
                ("readiness", 3, "Readiness"), ("publish", 4, "Finish"),
                ("complete", 4, "Finish"),
            ]),
        Mapping(
            kind: .stopInstance,
            keys: [
                ("validate", 0, "Check"), ("lock", 0, "Check"), ("journal", 1, "Prepare"),
                ("stop", 2, "Stop"), ("complete", 3, "Finish"),
            ]),
        Mapping(
            kind: .restartInstance,
            keys: [
                ("validate", 0, "Check"), ("lock", 0, "Check"), ("journal", 0, "Check"),
                ("stop", 1, "Stop"), ("await_exit", 1, "Stop"), ("lock_wait", 1, "Stop"),
                ("start", 2, "Start"), ("render", 2, "Start"), ("spawn", 2, "Start"),
                ("identity", 2, "Start"), ("backoff", 2, "Waiting to restart"),
                ("readiness", 3, "Readiness"), ("publish", 4, "Finish"),
                ("complete", 4, "Finish"),
            ]),
        Mapping(
            kind: .forceStopInstance,
            keys: [
                ("validate", 0, "Check"), ("lock", 0, "Check"), ("journal", 0, "Check"),
                ("kill", 1, "Kill"), ("await_exit", 2, "Wait for exit"),
                ("complete", 3, "Finish"),
            ]),
        Mapping(
            kind: .applyConfig,
            keys: [
                ("validate", 0, "Check"), ("lock", 0, "Check"), ("commit", 1, "Save revision"),
                ("apply", 2, "Apply"), ("verify", 3, "Verify"),
            ]),
        Mapping(
            kind: .changeEndpoints,
            keys: [
                ("validate", 0, "Check"), ("lock", 0, "Check"), ("commit", 1, "Save revision"),
                ("apply", 2, "Apply"), ("verify", 3, "Verify"),
            ]),
        Mapping(
            kind: .resolveConfigDrift,
            keys: [
                ("validate", 0, "Check"), ("render", 1, "Rewrite files"),
                ("verify", 2, "Verify"),
            ]),
    ]

    @Test(arguments: mappings)
    func everyKeyTheJobEntersMapsToTheIndexOfItsStage(_ mapping: Mapping) throws {
        let stages = try #require(OperationStages.definitions(for: mapping.kind))
        var lastIndex = 0
        for entry in mapping.keys {
            let operation = OperationSample.make(
                kind: mapping.kind, state: .running, stage: entry.key)
            let value = StagedProgressValue.from(operation: operation, stages: stages)
            #expect(value.runningIndex == entry.index, "\(entry.key)")
            #expect(value.currentTitle == entry.title, "\(entry.key)")
            #expect(OperationStages.label(for: operation) == entry.title, "\(entry.key)")
            // The job enters its keys in this order, so the bar never goes back.
            #expect(entry.index >= lastIndex, "\(entry.key)")
            lastIndex = entry.index
        }
        // Every stage of the definition is reached by some key.
        #expect(Set(mapping.keys.map(\.index)) == Set(stages.indices))
    }

    @Test(arguments: mappings)
    func aKeyNoDefinitionKnowsTakesTheFallbackLabel(_ mapping: Mapping) throws {
        let stages = try #require(OperationStages.definitions(for: mapping.kind))
        let operation = OperationSample.make(
            kind: mapping.kind, state: .running, stage: "warm_cache")
        let value = StagedProgressValue.from(operation: operation, stages: stages)
        #expect(value.runningIndex == nil)
        #expect(OperationStages.label(for: operation) == "Warm cache")
    }

    @Test func backoffWaitsInLaunchWithoutMovingTheBarBack() throws {
        let stages = try #require(OperationStages.definitions(for: .startInstance))
        let launching = StagedProgressValue.from(
            operation: OperationSample.make(state: .running, stage: "spawn"), stages: stages)
        let waiting = StagedProgressValue.from(
            operation: OperationSample.make(state: .running, stage: "backoff"), stages: stages)
        #expect(waiting.runningIndex == launching.runningIndex)
        #expect(waiting.fraction == launching.fraction)
        #expect(waiting.percent == launching.percent)
        #expect(waiting.currentTitle == "Waiting to restart")
        #expect(waiting.compactText == "Waiting to restart · 40%")
        // The stage list keeps the stage's own title.
        #expect(waiting.stages[2].title == "Launch")
        #expect(waiting.stages[2].state == .running)
    }

    @Test func aFailedOperationNamesTheStageOfItsAlias() throws {
        let stages = try #require(OperationStages.definitions(for: .restartInstance))
        let failed = StagedProgressValue.from(
            operation: OperationSample.make(
                kind: .restartInstance, state: .failed, stage: "await_exit"),
            stages: stages)
        #expect(failed.runningIndex == 1)
        #expect(failed.currentTitle == "Failed at Stop")
    }

    @Test func theKindsWithOnlyTwoStagesOrAnotherOwnerHaveNoDefinition() {
        for kind: OperationKind in [
            .reloadInstance, .renameInstance, .setInstanceStartup, .updateDaemonSettings,
            .stopAllInstances, .daemonRestart, .removeInstance, .purgeInstance,
            .backupInstance, .restoreInstance, .unrecognized,
        ] {
            #expect(OperationStages.definitions(for: kind) == nil, "\(kind)")
        }
        #expect(OperationStages.definitions(for: .installRuntime)?.count == 5)
        #expect(OperationStages.definitions(for: .createInstance)?.count == 6)
    }

    @Test func anAliasWithoutATitleShowsItsStageTitle() {
        let definitions = [
            StageDefinition(key: "a", title: "First"),
            StageDefinition(
                key: "b", title: "Second", also: ["c", StageAlias(key: "d", title: "Dee")]),
        ]
        #expect(StagedProgressValue.stage(of: "a", in: definitions)?.index == 0)
        #expect(StagedProgressValue.stage(of: "c", in: definitions)?.title == "Second")
        #expect(StagedProgressValue.stage(of: "d", in: definitions)?.title == "Dee")
        #expect(StagedProgressValue.stage(of: "d", in: definitions)?.index == 1)
        #expect(StagedProgressValue.stage(of: "e", in: definitions) == nil)
        #expect(StageDefinition(key: "x", title: "X").also.isEmpty)
    }

    // MARK: - Status and cancel (spec 0035, *Ketersediaan cancel*)

    @Test func aContinuingCancelShowsFinishingInsteadOfCancelling() {
        let finishing = StatusAppearance.operation(
            state: .cancelRequested, partial: false, continuing: true)
        #expect(finishing.label == "Finishing…")
        #expect(finishing.tone == .caution)
        #expect(
            StatusAppearance.operation(state: .cancelRequested, partial: false).label
                == "Cancelling…")
        for state: OperationStatus in [
            .queued, .running, .succeeded, .failed, .cancelled, .needsRecovery,
        ] {
            #expect(
                StatusAppearance.operation(state: state, partial: false, continuing: true)
                    == StatusAppearance.operation(state: state, partial: false))
        }
    }

    @Test(
        .evidence(
            covers: ["REQ-054"],
            expected:
                "the GUI uses the cancel availability the daemon sent, and without one falls back to available for queued and running, requested for cancel_requested, and none otherwise"
        ))
    func theCancelAvailabilityIsTheDaemonsOrTheFallbackRule() {
        let sent: [CancelAvailability] = [
            .available, .pastPointOfNoReturn, .requested, .continuing, .none,
        ]
        for value in sent {
            #expect(OperationCancel.availability(of: OperationSample.make(cancel: value)) == value)
        }
        let newer = OperationSample.make(cancel: .unrecognized)
        #expect(OperationCancel.availability(of: newer) == .none)
        let fallback: [(OperationState, CancelAvailability)] = [
            (.queued, .available), (.running, .available), (.cancelRequested, .requested),
            (.succeeded, .none), (.failed, .none), (.cancelled, .none), (.needsRecovery, .none),
        ]
        for (state, expected) in fallback {
            // The fallback never reads the point of no return.
            let operation = OperationSample.make(
                state: state, cancel: nil, pointOfNoReturnAt: OperationSample.base)
            #expect(OperationCancel.availability(of: operation) == expected, "\(state)")
        }
    }

    @Test func theBadgeOfAContinuingCancelSaysFinishing() {
        let continuing = OperationSample.make(state: .cancelRequested, cancel: .continuing)
        #expect(OperationCancel.appearance(of: continuing).label == "Finishing…")
        let requested = OperationSample.make(state: .cancelRequested, cancel: .requested)
        #expect(OperationCancel.appearance(of: requested).label == "Cancelling…")
        let older = OperationSample.make(state: .cancelRequested, cancel: nil)
        #expect(OperationCancel.appearance(of: older).label == "Cancelling…")
    }
}
