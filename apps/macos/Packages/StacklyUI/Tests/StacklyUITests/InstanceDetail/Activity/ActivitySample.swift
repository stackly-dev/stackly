import Foundation
import StacklyClient

@testable import StacklyUI

/// Builds the audit rows of the Activity tests (spec 0035, AC-15): every field the tests vary,
/// with a fixed clock so the order of `audit.list` is known.
enum ActivitySample {
    /// The instance every row is about.
    static let instance = UUID(uuidString: "0192f3a5-0000-7000-8000-000000000101") ?? UUID()

    /// The moment of row 1; row `n` happened `n` minutes later.
    static let base = Date(timeIntervalSince1970: 1_793_000_000)

    /// The ID of row `number`.
    static func id(_ number: UInt8) -> UUID {
        UUID(uuidString: String(format: "0192f3a5-0000-7000-8000-0000000000%02x", number))
            ?? UUID()
    }

    /// The ID of operation `number`.
    static func operation(_ number: UInt8) -> UUID {
        UUID(uuidString: String(format: "0192f3a5-0000-7000-8000-0000000002%02x", number))
            ?? UUID()
    }

    /// A row; the defaults are a start of the instance from the app that succeeded.
    static func row(
        _ number: UInt8 = 1,
        action: String = "operation.finished",
        kind: OperationKind? = .startInstance,
        result: AuditResultKind = .succeeded,
        actor: AuditActorKind = .user,
        client: ClientKind? = .gui,
        operation: UUID? = nil,
        configRevision: UInt64? = nil,
        details: [String: String] = [:],
        at: Date? = nil
    ) -> AuditEventInfo {
        let isOperation = action.hasPrefix("operation.")
        return AuditEventInfo(
            id: id(number), occurredAt: at ?? base.addingTimeInterval(Double(number) * 60),
            actor: actor, client: client, action: action,
            operationKind: isOperation ? kind : nil,
            operationId: operation ?? (isOperation ? Self.operation(number) : nil),
            resource: ResourceRef(kind: .instance, id: instance.uuidString.lowercased()),
            instanceId: instance, configRevision: configRevision, result: result,
            details: details.sorted { $0.key < $1.key }.map {
                AuditDetail(key: $0.key, value: $0.value)
            })
    }

    /// A page of `rows` with `next`.
    static func page(_ rows: [AuditEventInfo], next: String? = nil) -> AuditPage {
        AuditPage(items: rows, next: next)
    }
}
