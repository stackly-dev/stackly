import Foundation
import Observation
import StacklyClient
import SwiftUI

/// The model of the Ports sheet (spec 0029, AC-14, AC-19): the IP endpoints of an instance
/// as drafts, judged with the dry run of `instance.change_endpoints` after a pause, and sent
/// as one operation. A fixed port that conflicts is never replaced on its own; the verdict
/// offers a free port and the person takes it with a button.
@Observable
public final class EndpointsModel {
    /// Where the read of the endpoints stands.
    public enum Phase: Equatable, Sendable {
        /// The first read is on its way.
        case loading
        /// The endpoints are on screen.
        case loaded
        /// The read failed.
        case failed(ErrorBody)
    }

    /// The instance.
    public let source: InstanceEditSource
    /// Where the read stands.
    public private(set) var phase = Phase.loading
    /// The IP endpoints that can change, as drafts.
    public private(set) var endpoints: [EndpointDraft] = []
    /// The verdict on the ports typed, `nil` while nothing changed.
    public private(set) var verdict: EndpointValidation?
    /// Why the ports could not be judged.
    public private(set) var error: ErrorBody?
    /// Whether a verdict is on its way.
    public private(set) var isValidating = false
    /// Whether the change is on its way.
    public private(set) var isBusy = false

    @ObservationIgnored private let service: InstanceConfigService
    @ObservationIgnored private let sleep: @Sendable (Duration) async throws -> Void
    @ObservationIgnored private var baseline: [EndpointDraft] = []
    @ObservationIgnored private var baseRevision: UInt64 = 0
    @ObservationIgnored private var task: Task<Void, Never>?
    @ObservationIgnored private var verdictRequests = 0
    @ObservationIgnored private var unknown: String?

    /// Creates a model that has read nothing.
    /// - Parameters:
    ///   - source: The instance.
    ///   - service: The reads and the mutation path.
    ///   - sleep: Waits for a duration and throws when cancelled; the pause before a verdict.
    public init(
        source: InstanceEditSource, service: InstanceConfigService,
        sleep: @escaping @Sendable (Duration) async throws -> Void = {
            try await Task.sleep(for: $0)
        }
    ) {
        self.source = source
        self.service = service
        self.sleep = sleep
    }

    /// Reads the endpoints of the instance.
    public func load() async {
        phase = .loading
        do throws(ClientError) {
            let view = try await service.get(source.id)
            baseRevision = view.revision.number
            endpoints = view.endpoints.filter(\.changeable).map(Self.draft)
            baseline = endpoints
            verdict = nil
            phase = .loaded
        } catch {
            phase = .failed(error.body)
        }
    }

    private static func draft(_ endpoint: ConfigEndpoint) -> EndpointDraft {
        let request = endpoint.request ?? .auto
        var draft = EndpointDraft(
            name: endpoint.name, purpose: .client, host: endpoint.host, mode: .automatic,
            portText: "")
        if case .fixed(let port) = request {
            draft.mode = .fixed
            draft.portText = String(port)
        }
        return draft
    }

    // MARK: - Editing

    /// Sets Fixed or Automatic for an endpoint.
    public func setMode(_ mode: PortMode, of name: String) {
        update(name) { $0.mode = mode }
    }

    /// Sets the typed port of an endpoint.
    public func setPortText(_ text: String, of name: String) {
        update(name) { $0.portText = text }
    }

    /// Takes the free port the verdict offered, when the person asks for it.
    public func usePort(_ port: UInt16, for name: String) {
        update(name) {
            $0.mode = .fixed
            $0.portText = String(port)
        }
    }

    private func update(_ name: String, _ change: (inout EndpointDraft) -> Void) {
        guard let index = endpoints.firstIndex(where: { $0.name == name }) else { return }
        let before = endpoints[index]
        change(&endpoints[index])
        guard endpoints[index] != before else { return }
        schedule()
    }

    /// The endpoints whose port request differs from the one the instance has.
    public var changed: [EndpointDraft] {
        endpoints.filter { draft in baseline.first { $0.name == draft.name } != draft }
    }

    /// The requests of the changed endpoints; empty while one has an unusable port.
    public var pendingRequests: [EndpointRequest] {
        changed.compactMap(\.request)
    }

    private var hasInvalidPort: Bool { changed.contains { $0.hasInvalidPort } }

    // MARK: - Verdict

    private func schedule() {
        task?.cancel()
        verdictRequests += 1
        verdict = nil
        error = nil
        guard changed.isEmpty == false, hasInvalidPort == false else {
            isValidating = false
            return
        }
        isValidating = true
        let sleep = self.sleep
        task = Task { [weak self] in
            do { try await sleep(ConfigurationModel.validateDebounce) } catch { return }
            await self?.validateNow()
        }
    }

    /// Judges the ports now, without the pause.
    public func validateNow() async {
        guard changed.isEmpty == false, hasInvalidPort == false else { return }
        verdictRequests += 1
        let mine = verdictRequests
        do throws(ClientError) {
            let result = try await service.validateEndpoints(
                source.id, baseRevision, pendingRequests)
            guard mine == verdictRequests else { return }
            verdict = result
            isValidating = false
        } catch {
            guard mine == verdictRequests else { return }
            isValidating = false
            self.error = error.body
        }
    }

    /// The check under the row of an endpoint, from the verdict.
    /// - Parameter name: The endpoint.
    public func check(forEndpoint name: String) -> PreflightCheck? {
        guard let problem = verdict?.problems.first(where: { $0.endpoint == name }) else {
            return nil
        }
        return PreflightCheck(
            id: "endpoint.\(name)", kind: .endpoint, status: .error, title: name,
            detail: Self.text(for: problem.code), subject: name,
            suggestedPort: problem.suggestedPort, error: nil)
    }

    /// The sentence for a code of a port that cannot be used.
    static func text(for code: String) -> String {
        switch code {
        case "supervisor.endpoint_conflict": "Another Stackly instance uses this port."
        case "supervisor.endpoint_in_use": "Another process on this Mac uses this port."
        case "supervisor.endpoint_unavailable": "This port can’t be used on this Mac."
        case "instance.endpoint_invalid": "Use a port from 1 to 65535."
        default: "This port can’t be used."
        }
    }

    /// The line under the rows: what applying does, or `nil` before a verdict.
    public var impactNote: String? {
        guard let verdict, verdict.valid, verdict.changes.isEmpty == false else { return nil }
        switch verdict.impact {
        case .restart: return "\(source.name) restarts briefly on the new ports."
        case .nextStart: return "The new ports apply at next start."
        case .none, .reload, .unrecognized: return nil
        }
    }

    /// Why the main button is off, or `nil` when the ports can change.
    public var disabledReason: String? {
        if phase != .loaded { return "Reading the ports…" }
        if changed.isEmpty { return "Change a port first." }
        if hasInvalidPort { return EndpointDraft.invalidPortMessage }
        if isValidating || (verdict == nil && error == nil) { return "Checking the ports…" }
        if error != nil { return "The ports could not be checked." }
        guard let verdict else { return nil }
        if verdict.valid == false { return "Fix the ports first." }
        if verdict.changes.isEmpty { return "Change a port first." }
        return nil
    }

    // MARK: - Change

    /// Changes the ports and follows the operation to its end.
    /// - Parameter context: Where the cards go.
    /// - Returns: Whether the sheet may close.
    public func apply(context: InstanceActionContext) async -> Bool {
        guard isBusy == false, disabledReason == nil else { return false }
        isBusy = true
        defer { isBusy = false }
        let call = Call.instanceChangeEndpoints(
            ChangeEndpointsParams(
                instanceId: source.id, baseRevision: baseRevision, endpoints: pendingRequests,
                dryRun: false))
        let key = unknown ?? MutationPath.newKey()
        let end = await ConfigMutationRunner(service: service).run(call, key: key)
        unknown = nil
        switch end {
        case .succeeded:
            context.report(.success("Ports changed on \(source.name)."))
            return true
        case .unchanged:
            context.report(.neutral("Nothing to change on \(source.name)."))
            return true
        case .refused(let body):
            if body.code == "config.revision_conflict" {
                await load()
                context.report(
                    .neutral(
                        "This configuration changed.",
                        detail: "The ports on screen are read again. Check them, then change."))
            } else {
                context.report(.failure(body))
                if body.code == "config.drift_detected" { return true }
            }
            return false
        case .failed(let body, _):
            context.report(
                Feedback(
                    tone: .critical, message: "Couldn’t change the ports of \(source.name).",
                    detail: body.map { DaemonText.sentence($0.message) }, code: body?.code))
            return true
        case .cancelled:
            context.report(.neutral("Changing the ports of \(source.name) was cancelled."))
            return true
        case .notCompleted(let outcome):
            if case .outcomeUnknown(_, let key) = outcome { unknown = key }
            if let card = Feedback.outcome(
                outcome, success: nil, showOperations: context.showOperations)
            {
                context.report(card)
            }
            return false
        }
    }
}

/// The Ports sheet: one `EndpointPortRow` per endpoint that can change, the verdict under
/// each, and what applying does.
struct EndpointsSheet: View {
    @Environment(\.dismiss) private var dismiss
    @State private var model: EndpointsModel

    let context: InstanceActionContext

    /// The sheet of an instance, with a model of its own.
    init(source: InstanceEditSource, service: InstanceConfigService, context: InstanceActionContext)
    {
        _model = State(initialValue: EndpointsModel(source: source, service: service))
        self.context = context
    }

    /// The sheet over a model made elsewhere, for a preview or a snapshot.
    init(model: EndpointsModel, context: InstanceActionContext) {
        _model = State(initialValue: model)
        self.context = context
    }

    var body: some View {
        StacklySheet(
            title: "Edit Ports", subtitle: model.source.name, primary: primary, minWidth: 520
        ) {
            VStack(alignment: .leading, spacing: Spacing.s12) {
                switch model.phase {
                case .loading:
                    ProgressView()
                        .controlSize(.small)
                        .frame(maxWidth: .infinity)
                case .failed(let body):
                    Label(body.message, systemImage: Tone.critical.symbol)
                        .foregroundStyle(Tone.critical.text)
                case .loaded:
                    rows
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .task { await model.load() }
    }

    @ViewBuilder private var rows: some View {
        if model.endpoints.isEmpty {
            Text("This instance has no ports you can change.")
                .foregroundStyle(StacklyColor.textSecondary)
        }
        ForEach(model.endpoints) { endpoint in
            EndpointPortRow(
                endpoint: endpoint, check: model.check(forEndpoint: endpoint.name),
                setMode: { model.setMode($0, of: endpoint.name) },
                setPortText: { model.setPortText($0, of: endpoint.name) },
                usePort: { model.usePort($0, for: endpoint.name) })
        }
        if let error = model.error {
            Label(error.message, systemImage: Tone.critical.symbol)
                .font(.callout)
                .foregroundStyle(Tone.critical.text)
        }
        if let note = model.impactNote {
            Text(note)
                .font(.callout)
                .foregroundStyle(StacklyColor.textSecondary)
        }
    }

    private var primary: StacklyAction {
        StacklyAction(
            id: "ports.confirm",
            title: model.isBusy ? ConfigurationCopy.changingPorts : "Change Ports",
            mutates: true, disabledReason: model.isBusy ? nil : model.disabledReason,
            isBusy: model.isBusy,
            perform: {
                Task {
                    if await model.apply(context: context) { dismiss() }
                }
            })
    }
}
