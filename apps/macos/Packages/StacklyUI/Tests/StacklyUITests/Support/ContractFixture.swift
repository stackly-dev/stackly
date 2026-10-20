import Foundation
import StacklyClient
import Testing

/// The shared contract fixtures in `protocols/fixtures/ipc/`, the same files the Rust and
/// client tests read, decoded with ``StacklyJSON``.
enum ContractFixture {
    /// A fixture that is not the frame a test expected.
    struct Mismatch: Error, CustomStringConvertible {
        let description: String
    }

    /// The server frame in `ipc/<name>.json`.
    static func frame(_ name: String) throws -> ServerFrame {
        let url = SourceTree.url(of: "protocols/fixtures/ipc/\(name).json")
        return try StacklyJSON.decoder().decode(ServerFrame.self, from: Data(contentsOf: url))
    }

    /// The reply of the response frame in `ipc/<name>.json`.
    static func reply(_ name: String) throws -> Reply {
        guard case .response(let response) = try frame(name) else {
            throw Mismatch(description: "\(name) is not a response frame")
        }
        return response.reply
    }

    /// The error body of the error frame in `ipc/<name>.json`.
    static func error(_ name: String) throws -> ErrorBody {
        guard case .error(let frame) = try frame(name) else {
            throw Mismatch(description: "\(name) is not an error frame")
        }
        return frame.error
    }

    /// The event of the event frame in `ipc/<name>.json`.
    static func event(_ name: String) throws -> Event {
        guard case .event(let event) = try frame(name) else {
            throw Mismatch(description: "\(name) is not an event frame")
        }
        return event
    }

    /// The page of the `operation.list` fixture.
    static func operationPage() throws -> OperationPage {
        guard case .operationList(let page) = try reply("frame-response-operation-list") else {
            throw Mismatch(description: "the operation.list fixture holds another reply")
        }
        return page
    }

    /// The operation of an `operation.state_changed` or `operation.progress` fixture.
    static func operation(inEvent name: String) throws -> OperationSummary {
        switch try event(name).kind {
        case .operationStateChanged(let operation), .operationProgress(let operation):
            return operation
        default:
            throw Mismatch(description: "\(name) is not an operation event")
        }
    }
}

extension OperationSummary {
    /// A copy with another state, revision, and result.
    func with(
        state: OperationState? = nil,
        revision: UInt64,
        result: OperationResult?? = nil
    ) -> OperationSummary {
        OperationSummary(
            id: id, kind: kind, resource: resource, state: state ?? self.state, stage: stage,
            progress: progress, waitingOn: waitingOn, cancelRequestedAt: cancelRequestedAt,
            pointOfNoReturnAt: pointOfNoReturnAt, failure: failure, recovery: recovery,
            result: result ?? self.result, trigger: trigger, createdAt: createdAt,
            startedAt: startedAt, updatedAt: updatedAt, revision: revision)
    }
}

extension Event {
    /// An `operation.state_changed` or `operation.progress` event for `operation`.
    static func operation(
        _ operation: OperationSummary,
        progress: Bool = false,
        sequence: UInt64 = 1
    ) -> Event {
        Event(
            subscriptionId: 1, sequence: sequence, at: operation.updatedAt,
            resource: ResourceRef(kind: .operation, id: operation.id.uuidString.lowercased()),
            revision: operation.revision,
            kind: progress ? .operationProgress(operation) : .operationStateChanged(operation))
    }
}

// MARK: - Instances

extension ContractFixture {
    /// The instance of the `instance.get` family fixture `name`.
    static func instance(_ name: String = "frame-response-instance-get") throws -> InstanceSummary {
        guard case .instanceGet(let instance) = try reply(name) else {
            throw Mismatch(description: "\(name) is not an instance.get reply")
        }
        return instance
    }

    /// The page of the `instance.list` fixture `name`.
    static func instancePage(_ name: String = "frame-response-instance-list") throws
        -> InstancePage
    {
        guard case .instanceList(let page) = try reply(name) else {
            throw Mismatch(description: "\(name) is not an instance.list reply")
        }
        return page
    }

    /// The `instance.health_changed` content of the event fixture `name`.
    static func healthChange(_ name: String) throws -> (event: Event, change: InstanceHealthChanged)
    {
        let event = try event(name)
        guard case .instanceHealthChanged(let change) = event.kind else {
            throw Mismatch(description: "\(name) is not an instance.health_changed event")
        }
        return (event, change)
    }
}

extension InstanceSummary {
    /// A copy with other values; every other field stays.
    func testing(
        id: UUID? = nil,
        name: String? = nil,
        revision: UInt64? = nil,
        lifecycle: InstanceLifecycle? = nil,
        readiness: InstanceReadiness? = nil,
        health: InstanceHealth? = nil,
        statusSeq: UInt64? = nil
    ) -> InstanceSummary {
        InstanceSummary(
            id: id ?? self.id, name: name ?? self.name, displayName: displayName,
            revision: revision ?? self.revision, software: software, provider: provider,
            runtimeId: runtimeId, runtimeVersion: runtimeVersion,
            lifecycle: lifecycle ?? self.lifecycle, desired: desired,
            readiness: readiness ?? self.readiness, health: health ?? self.health,
            statusSeq: statusSeq ?? self.statusSeq, process: process, lastFailure: lastFailure,
            restart: restart, blocker: blocker, endpoints: endpoints,
            leftoverProcesses: leftoverProcesses, logsUnavailable: logsUnavailable,
            ownership: ownership, initialized: initialized)
    }

    /// The `instance.health_changed` content that carries this summary's status fields at
    /// `statusSeq`, with another health state.
    func healthChange(statusSeq: UInt64, health state: InstanceHealthState) -> InstanceHealthChanged
    {
        InstanceHealthChanged(
            instanceId: id, statusSeq: statusSeq, readiness: readiness,
            health: InstanceHealth(
                state: state, reason: "test", detail: nil, observedAt: health.observedAt),
            blocker: blocker, restart: restart, process: process,
            logsUnavailable: logsUnavailable)
    }
}

extension Event {
    /// An `instance.state_changed` event for `instance`, at the instance's revision.
    static func instanceState(_ instance: InstanceSummary, sequence: UInt64 = 1) -> Event {
        Event(
            subscriptionId: 1, sequence: sequence, at: Date(timeIntervalSince1970: 0),
            resource: ResourceRef(kind: .instance, id: instance.id.uuidString.lowercased()),
            revision: instance.revision,
            kind: .instanceStateChanged(InstanceStateChanged(instance: instance)))
    }

    /// An `instance.health_changed` event at `revision`.
    static func instanceHealth(
        _ change: InstanceHealthChanged, revision: UInt64, sequence: UInt64 = 1
    ) -> Event {
        Event(
            subscriptionId: 1, sequence: sequence, at: Date(timeIntervalSince1970: 0),
            resource: ResourceRef(kind: .instance, id: change.instanceId.uuidString.lowercased()),
            revision: revision, kind: .instanceHealthChanged(change))
    }

    /// An `instance.removed` event at `revision`.
    static func instanceRemoved(_ id: UUID, revision: UInt64, sequence: UInt64 = 1) -> Event {
        Event(
            subscriptionId: 1, sequence: sequence, at: Date(timeIntervalSince1970: 0),
            resource: ResourceRef(kind: .instance, id: id.uuidString.lowercased()),
            revision: revision, kind: .instanceRemoved(InstanceRemoved(instanceId: id)))
    }
}
