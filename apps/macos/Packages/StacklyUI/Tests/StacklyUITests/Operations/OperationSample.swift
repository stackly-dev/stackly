import Foundation
import StacklyClient

@testable import StacklyUI

/// Builds the operation summaries of the Operations tests (spec 0035): every field the tests
/// vary, with a fixed clock so the order of `operation.list` is known.
enum OperationSample {
    /// The creation time of operation 1; operation `n` is created `n` seconds later.
    static let base = Date(timeIntervalSince1970: 1_793_000_000)

    /// The ID of operation `number`, a lowercase UUID v7 like the daemon's.
    static func id(_ number: UInt8) -> UUID {
        UUID(uuidString: String(format: "0192f3a4-0000-7000-8000-0000000000%02x", number))
            ?? UUID()
    }

    /// The ID of instance `number`.
    static func instance(_ number: UInt8) -> UUID {
        UUID(uuidString: String(format: "0192f3a4-0000-7000-8000-0000000001%02x", number))
            ?? UUID()
    }

    /// An operation; the defaults are a running start of instance 1.
    static func make(
        _ number: UInt8 = 1,
        kind: OperationKind = .startInstance,
        instance: UUID? = OperationSample.instance(1),
        state: OperationState = .running,
        stage: String? = nil,
        progress: OperationProgress = .indeterminate,
        waitingOn: UUID? = nil,
        cancel: CancelAvailability? = nil,
        cancelRequestedAt: Date? = nil,
        pointOfNoReturnAt: Date? = nil,
        failure: OperationFailureInfo? = nil,
        recovery: RecoveryInfo? = nil,
        result: OperationResult? = nil,
        trigger: OperationTriggerInfo = OperationTriggerInfo(origin: .user, client: .gui),
        createdAt: Date? = nil,
        revision: UInt64 = 1
    ) -> OperationSummary {
        let created = createdAt ?? base.addingTimeInterval(TimeInterval(number))
        return OperationSummary(
            id: id(number), kind: kind,
            resource: instance.map { ResourceRef(kind: .instance, id: $0.uuidString.lowercased()) },
            state: state, stage: stage, progress: progress, waitingOn: waitingOn,
            cancelRequestedAt: cancelRequestedAt, pointOfNoReturnAt: pointOfNoReturnAt,
            failure: failure, recovery: recovery, result: result, trigger: trigger,
            createdAt: created, startedAt: state == .queued ? nil : created,
            updatedAt: created, revision: revision, cancel: cancel, parent: nil)
    }

    /// A failure of `code` with `message`.
    static func failure(_ code: String, _ message: String) -> OperationFailureInfo {
        OperationFailureInfo(
            reason: "step_failed", retryable: true,
            error: ErrorBody(
                code: code, category: .failed, message: message, retryable: true,
                resource: nil, fields: nil, hint: nil, details: nil, operationId: nil, rule: nil,
                items: nil))
    }

    /// A recovery with `hint` and `actions`.
    static func recovery(_ hint: RecoveryHint, actions: [RecoveryAction]) -> RecoveryInfo {
        RecoveryInfo(hint: hint, cause: .stepFailed, error: nil, actions: actions)
    }
}
