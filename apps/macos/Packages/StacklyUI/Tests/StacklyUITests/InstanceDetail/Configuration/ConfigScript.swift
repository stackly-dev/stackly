import Foundation
import StacklyClient
import Synchronization

@testable import StacklyUI

/// A scripted daemon for the Configuration tab (spec 0029): it records every read and every
/// mutation, answers `instance.config.get` from a queue (the last answer repeats), and judges
/// every change with the verdict it was given.
nonisolated final class ConfigScript: Sendable {
    private struct State {
        var view: InstanceConfigView
        var views: [InstanceConfigView] = []
        var verdict: Result<ConfigValidation, ClientError>
        var validated: [ConfigChangeRequest] = []
        var sent: [(call: Call, key: String)] = []
        var outcome: MutationOutcome
        var final: OperationSummary?
        var endpointVerdict: Result<EndpointValidation, ClientError> = .success(
            EndpointValidation(
                valid: true, problems: [],
                changes: [EndpointChange(name: "main", old: .fixed(5555), new: .fixed(5600))],
                impact: .restart, leases: nil))
        var validatedEndpoints: [[EndpointRequest]] = []
    }

    static let instance = UUID(uuidString: "0192f3a5-0000-7000-8000-000000000001") ?? UUID()

    private let state: Mutex<State>

    @MainActor init(view: InstanceConfigView) throws {
        state = Mutex(
            State(
                view: view, verdict: .success(try Self.verdict("restart")),
                outcome: .completed(
                    try ContractFixture.reply("frame-response-instance-config-apply"))
            ))
    }

    /// The config view of the fake provider fixture: revision 2, ready, reload supported.
    @MainActor static func fakeView() throws -> InstanceConfigView {
        guard
            case .instanceConfigGet(let view) = try ContractFixture.reply(
                "frame-response-instance-config-get-fake")
        else { throw ContractFixture.Mismatch(description: "not a config view") }
        return view
    }

    /// The verdict fixture `frame-response-instance-config-validate-<name>`.
    @MainActor static func verdict(_ name: String) throws -> ConfigValidation {
        guard
            case .instanceConfigValidate(let verdict) = try ContractFixture.reply(
                "frame-response-instance-config-validate-\(name)")
        else { throw ContractFixture.Mismatch(description: "not a verdict") }
        return verdict
    }

    /// The operation of the apply fixture in the state `state`.
    @MainActor static func operation(_ state: String) throws -> OperationSummary {
        let url = SourceTree.url(
            of: "protocols/fixtures/ipc/frame-response-instance-config-apply.json")
        let text = try String(contentsOf: url, encoding: .utf8)
            .replacingOccurrences(of: "\"state\": \"queued\"", with: "\"state\": \"\(state)\"")
        let frame = try StacklyJSON.decoder().decode(ServerFrame.self, from: Data(text.utf8))
        guard case .response(let response) = frame,
            case .instanceConfigApply(let accepted) = response.reply,
            let operation = accepted.accepted?.operation
        else { throw ContractFixture.Mismatch(description: "not an apply reply") }
        return operation
    }

    var validated: [ConfigChangeRequest] { state.withLock { $0.validated } }
    var sent: [(call: Call, key: String)] { state.withLock { $0.sent } }
    var validatedEndpoints: [[EndpointRequest]] { state.withLock { $0.validatedEndpoints } }

    func judgeEndpoints(_ verdict: Result<EndpointValidation, ClientError>) {
        state.withLock { $0.endpointVerdict = verdict }
    }

    func show(_ view: InstanceConfigView) { state.withLock { $0.view = view } }
    func judge(_ verdict: Result<ConfigValidation, ClientError>) {
        state.withLock { $0.verdict = verdict }
    }
    func answer(_ outcome: MutationOutcome, ending: OperationSummary?) {
        state.withLock {
            $0.outcome = outcome
            $0.final = ending
        }
    }

    var service: InstanceConfigService {
        InstanceConfigService(
            get: { [self] _ throws(ClientError) in state.withLock { $0.view } },
            validate: { [self] _, _, change throws(ClientError) in
                try state.withLock { state in
                    state.validated.append(change)
                    return state.verdict
                }.get()
            },
            validateEndpoints: { [self] _, _, requests throws(ClientError) in
                try state.withLock { state in
                    state.validatedEndpoints.append(requests)
                    return state.endpointVerdict
                }.get()
            },
            send: { [self] call, key in
                state.withLock { state in
                    state.sent.append((call, key))
                    return state.outcome
                }
            },
            updates: { [self] _ in
                let final = state.withLock { $0.final }
                return AsyncStream { continuation in
                    if let final { continuation.yield(final) }
                    continuation.finish()
                }
            },
            readOperation: { [self] _ in state.withLock { $0.final } })
    }
}

extension InstanceConfigView {
    /// The same view with the given parts replaced.
    func with(
        revision: ConfigRevisionSummary? = nil, lastKnownGood: UInt64?? = nil,
        history: [ConfigRevisionSummary]? = nil, drift: ConfigDrift?? = nil
    ) -> InstanceConfigView {
        InstanceConfigView(
            revision: revision ?? self.revision, lastKnownGood: lastKnownGood ?? self.lastKnownGood,
            fields: fields, endpoints: endpoints, history: history ?? self.history,
            effectiveFiles: effectiveFiles, effectiveFilesNote: effectiveFilesNote,
            drift: drift ?? self.drift, reloadSupported: reloadSupported)
    }
}

extension ConfigRevisionSummary {
    /// The same revision with the given parts replaced.
    func with(
        number: UInt64? = nil, readyAt: Date?? = nil, isActive: Bool? = nil,
        isLastKnownGood: Bool? = nil, values: JSONObject? = nil
    ) -> ConfigRevisionSummary {
        ConfigRevisionSummary(
            number: number ?? self.number, createdAt: createdAt, origin: origin,
            sourceNumber: sourceNumber, readyAt: readyAt ?? self.readyAt,
            isActive: isActive ?? self.isActive,
            isLastKnownGood: isLastKnownGood ?? self.isLastKnownGood, values: values ?? self.values,
            endpoints: endpoints)
    }
}
