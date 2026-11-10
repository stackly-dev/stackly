import Foundation
import StacklyClient
import StacklyTestSupport
import Testing

@testable import StacklyUI

/// The buttons of the failure banner (spec 0033, AC-10, AC-13, AC-16): Retry is the main
/// Retry of spec 0022, Use Port opens the Ports sheet filled in, Show Logs selects the Logs
/// tab, and Show in Operations and Diagnose… go through the navigator (provisional targets
/// until features 34 and 35).
@MainActor
@Suite(.timeLimit(.minutes(1)))
struct InstanceFailureBannerTests {
    /// What the daemon was sent and what the cards said.
    final class Script {
        var sent: [(call: Call, key: String)] = []
        var feedback: [Feedback] = []
        var holdNextSend = false
        var gate: CheckedContinuation<Void, Never>?

        func send(_ call: Call, _ key: String) async -> MutationOutcome {
            sent.append((call, key))
            if holdNextSend {
                holdNextSend = false
                await withCheckedContinuation { gate = $0 }
            }
            return .rejected(
                ErrorBody(
                    code: "supervisor.endpoint_in_use", category: .conflict,
                    message: "port 5432 of endpoint database is in use by another process",
                    retryable: false, resource: nil, fields: nil, hint: nil, details: nil,
                    operationId: nil, rule: nil, items: nil))
        }
    }

    private let script = Script()

    private func actionModel() -> InstanceActionModel {
        let script = script
        return InstanceActionModel(
            send: { await script.send($0, $1) },
            updates: { _ in AsyncStream { $0.finish() } },
            readOperation: { _ in nil },
            now: { FixtureClock.now })
    }

    private var context: InstanceActionContext {
        let script = script
        return InstanceActionContext(report: { script.feedback.append($0) }, showOperations: {})
    }

    private func content(_ fixture: FixtureFailureBanner) throws -> FailureBannerContent {
        try #require(
            FailureBannerContent.resolve(
                fixture.instance, related: fixture.operations, instanceNames: { _ in nil },
                now: FixtureClock.now))
    }

    private func actions(
        _ fixture: FixtureFailureBanner, main: StacklyAction, edit: InstanceEditAction? = nil,
        navigator: ShellNavigator? = nil, showLogs: (@MainActor () -> Void)? = nil
    ) -> FailureBannerActions {
        FailureBannerActions(
            summary: fixture.instance, main: main, edit: edit, navigator: navigator,
            showLogs: showLogs)
    }

    // MARK: - Retry

    @Test(
        .evidence(
            covers: ["AC-068"],
            expected:
                "Retry in the banner is the main Retry: each click sends one new instance.start with a new key and a refusal shows the failure card"
        ))
    func retrySendsANewStartWithANewKeyEachClick() async throws {
        // covers: spec 0033 AC-13 (Retry is one new instance.start with a new key)
        let model = actionModel()
        let summary = FixtureFailureBanner.crashLoop.instance
        for round in 1...2 {
            let retry = actions(
                .crashLoop, main: model.stacklyAction(for: summary, context: context)
            ).retry
            #expect(retry.title == "Retry")
            #expect(retry.mutates)
            #expect(retry.disabledReason == nil)
            retry.perform()
            while script.sent.count < round || model.localStatus(for: summary.id) != .none {
                await Task.yield()
            }
        }
        #expect(script.sent.count == 2)
        for sent in script.sent {
            guard case .instanceStart(let params) = sent.call else {
                Issue.record("not instance.start")
                return
            }
            #expect(params.instanceId == summary.id)
        }
        #expect(script.sent[0].key != script.sent[1].key)
        #expect(script.feedback.count == 2)
        #expect(script.feedback.allSatisfy { $0.tone == .critical })
    }

    @Test func retryShowsStartingWhileTheStartRuns() async throws {
        // covers: spec 0033 AC-13 (the busy label Starting…)
        let model = actionModel()
        let summary = FixtureFailureBanner.crashLoop.instance
        script.holdNextSend = true
        actions(.crashLoop, main: model.stacklyAction(for: summary, context: context)).retry
            .perform()
        while script.gate == nil { await Task.yield() }
        let busy = actions(.crashLoop, main: model.stacklyAction(for: summary, context: context))
            .retry
        #expect(busy.isBusy)
        #expect(busy.title == "Starting…")
        script.gate?.resume()
    }

    @Test func retryOnARefusedStoppedInstanceIsItsStartUnderTheBannerTitle() {
        let model = actionModel()
        let summary = FixtureFailureBanner.startRefused.instance
        let main = model.stacklyAction(for: summary, context: context)
        #expect(main.title == "Start")
        let retry = actions(.startRefused, main: main).retry
        #expect(retry.title == "Retry")
        #expect(retry.symbol == nil)
    }

    @Test(
        .evidence(
            covers: ["AC-068"],
            expected:
                "Retry keeps the reason the main action is off, so a start that cannot run is disabled with the reason of spec 0022"
        ))
    func retryKeepsTheReasonTheMainActionIsOff() {
        // covers: spec 0033 AC-13 (off with the same reason)
        let main = StacklyAction(
            id: "retry", title: "Retry", mutates: true,
            disabledReason: "Initialization failed. Remove the instance and create it again.",
            perform: {})
        let retry = actions(.crashLoop, main: main).retry
        #expect(retry.disabledReason == main.disabledReason)
        #expect(
            retry.availability(
                gate: .blocked(reason: "stacklyd is offline", since: FixtureClock.now))
                != .enabled)
    }

    // MARK: - Buttons

    @Test(
        .evidence(
            covers: ["AC-010"],
            expected:
                "The banner of a port in use offers Use Port, Show Logs, Retry, and Diagnose…, and Use Port opens the Ports sheet with the port filled in"
        ))
    func usePortOpensThePortsSheetFilledIn() throws {
        // covers: spec 0033 AC-10 (Use Port opens the Ports sheet filled in)
        var opened: [InstanceEdit] = []
        let edit = InstanceEditAction { opened.append($0) }
        let banner = InstanceFailureBanner(
            content: try content(.portInUse), mode: .summary,
            actions: actions(
                .portInUse, main: StacklyAction(id: "retry", title: "Retry", mutates: true) {},
                edit: edit, showLogs: {}))
        #expect(
            banner.buttons.map(\.title) == ["Use Port 5439", "Show Logs", "Retry", "Diagnose…"])
        banner.buttons.first?.perform()
        let source = InstanceEditSource(
            id: FixtureFailureBanner.portInUse.id,
            name: FixtureFailureBanner.portInUse.instance.name)
        #expect(opened == [.editPorts(source, prefill: ["database": 5439])])
    }

    @Test(
        .evidence(
            covers: ["AC-068"],
            expected: "The inspector shows only Use Port, and nothing for a banner without one"
        ))
    func theInspectorShowsOnlyUsePort() throws {
        // covers: spec 0033 AC-12 (the compact inspector banner)
        let edit = InstanceEditAction { _ in }
        let main = StacklyAction(id: "retry", title: "Retry", mutates: true) {}
        let port = InstanceFailureBanner(
            content: try content(.portInUse), mode: .inspector,
            actions: actions(.portInUse, main: main, edit: edit))
        #expect(port.buttons.map(\.title) == ["Use Port 5439"])
        let crash = InstanceFailureBanner(
            content: try content(.crashLoop), mode: .inspector,
            actions: actions(.crashLoop, main: main, edit: edit))
        #expect(crash.buttons.isEmpty)
    }

    @Test func aPlaceWithoutThePortsSheetOrTheLogsTabLeavesThoseButtonsOut() throws {
        let main = StacklyAction(id: "retry", title: "Retry", mutates: true) {}
        let banner = InstanceFailureBanner(
            content: try content(.portInUse), mode: .summary,
            actions: actions(.portInUse, main: main))
        #expect(banner.buttons.map(\.title) == ["Retry", "Diagnose…"])
    }

    @Test func theSentencesJoinTheMessageTheHintAndThePortNote() throws {
        let text = InstanceFailureBanner.sentences(of: try content(.portsChanged))
        #expect(text.hasPrefix("Port 5444 of database is held by another process. Stackly never"))
        #expect(text.hasSuffix(" Its ports changed. Retry to start with them."))
        #expect(InstanceFailureBanner.sentences(of: try content(.failed)) == "Stopped by signal 9.")
    }

    // MARK: - Navigation

    @Test(
        .evidence(
            covers: ["AC-068"],
            expected:
                "Show Logs selects the Logs tab, Show in Operations opens Operations, and Diagnose… opens Diagnostics focused on the instance, through the navigator"
        ))
    func theLinksGoToLogsOperationsAndDiagnostics() throws {
        // covers: spec 0033 AC-16 (Show Logs, Show in Operations, Diagnose…)
        let navigator = ShellNavigator(section: .instances)
        var showedLogs = 0
        let actions = actions(
            .crashLoop, main: StacklyAction(id: "retry", title: "Retry", mutates: true) {},
            navigator: navigator, showLogs: { showedLogs += 1 })

        try #require(actions.stacklyAction(for: .showLogs)).perform()
        #expect(showedLogs == 1)
        #expect(navigator.section == .instances)

        // Diagnostics with the Doctor section filtered to this instance (spec 0034, AC-20).
        try #require(actions.stacklyAction(for: .diagnose)).perform()
        #expect(navigator.section == .diagnostics)
        let instance = FixtureFailureBanner.crashLoop.instance.id
        #expect(navigator.diagnosticsRequest?.focus == .instance(instance))

        // The operation itself on the Operations page (spec 0035, AC-13).
        let operation = try #require(try content(.crashLoop).operationId)
        actions.showOperation(operation)
        #expect(navigator.section == .operations)
        #expect(navigator.operationRequest?.operation == operation)
    }
}
