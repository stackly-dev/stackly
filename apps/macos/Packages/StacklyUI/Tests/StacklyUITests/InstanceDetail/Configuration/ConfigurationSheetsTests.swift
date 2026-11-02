import Foundation
import StacklyClient
import StacklyTestSupport
import Testing

@testable import StacklyUI

/// The Rename and Ports sheets and the places that open them (spec 0029, AC-13, AC-14,
/// AC-19): the live name check, the port verdict with its suggestion, the mutation calls, and
/// the commands and the Overview item.
@MainActor
@Suite(.timeLimit(.minutes(1)))
struct ConfigurationSheetsTests {
    private let sleeper = ManualSleep()
    private let reported = ConfigurationModelTests.Reported()
    private let source = InstanceEditSource(id: ConfigScript.instance, name: "orders")

    private var context: InstanceActionContext {
        let reported = reported
        return InstanceActionContext(report: { reported.cards.append($0) })
    }

    private func rename(_ script: ConfigScript, others: Set<String> = ["billing"]) -> RenameModel {
        RenameModel(source: source, service: script.service, others: { others })
    }

    private func endpoints(_ script: ConfigScript) -> EndpointsModel {
        let sleeper = sleeper
        return EndpointsModel(
            source: source, service: script.service, sleep: { try await sleeper.sleep(for: $0) })
    }

    private func script() throws -> ConfigScript {
        try ConfigScript(view: ConfigScript.fakeView())
    }

    // MARK: - Rename

    @Test(
        .evidence(
            covers: ["BR-003", "VAL-001"],
            expected:
                "The name is checked as it is typed against the format and the other instances"
        ))
    func theNameIsCheckedAsItIsTyped() throws {
        let model = rename(try script())
        #expect(model.disabledReason == "Enter a new name.")
        #expect(model.problem == nil)

        model.name = "Orders 2"
        #expect(
            model.problem
                == "Use 1 to 63 lowercase letters, digits, or hyphens, starting with a letter.")
        model.name = "billing"
        #expect(model.problem == "An instance named billing already exists.")
        #expect(model.disabledReason == "An instance named billing already exists.")
        model.name = "orders-2"
        #expect(model.problem == nil)
        #expect(model.disabledReason == nil)
    }

    @Test func theNameRuleMatchesTheFormatOfAnInstanceName() {
        #expect(RenameModel.isValid("pg17-dev"))
        #expect(RenameModel.isValid("a"))
        #expect(RenameModel.isValid(String(repeating: "a", count: 63)))
        #expect(RenameModel.isValid(String(repeating: "a", count: 64)) == false)
        #expect(RenameModel.isValid("1db") == false)
        #expect(RenameModel.isValid("Db") == false)
        #expect(RenameModel.isValid("") == false)
        #expect(RenameModel.isValid("a_b") == false)
    }

    @Test(
        .evidence(
            covers: ["BR-003"],
            expected: "Rename sends the new name and reports it, and the sheet closes"
        ))
    func renameSendsTheNameAndReportsIt() async throws {
        let script = try script()
        script.answer(
            .completed(try ContractFixture.reply("frame-response-instance-rename")),
            ending: try ConfigScript.operation("succeeded"))
        let model = rename(script)
        model.name = " orders-2 "
        let closes = await model.submit(context: context)

        #expect(closes)
        guard case .instanceRename(let params)? = script.sent.first?.call else {
            Issue.record("expected a rename call")
            return
        }
        #expect(params.name == "orders-2")
        #expect(params.instanceId == ConfigScript.instance)
        #expect(reported.cards.map(\.message) == ["orders renamed to orders-2."])
    }

    @Test func aNameTakenInTheMeantimeStaysOnTheSheetWithItsSentence() async throws {
        let script = try script()
        script.answer(.rejected(Self.refusal("instance.name_taken")), ending: nil)
        let model = rename(script)
        model.name = "orders-2"
        let closes = await model.submit(context: context)

        #expect(closes == false)
        #expect(model.problem == "An instance named orders-2 already exists.")
        model.edited()
        #expect(model.problem == nil)
        #expect(reported.cards.isEmpty)
    }

    @Test func aLostAnswerKeepsTheSheetAndReusesTheKey() async throws {
        let script = try script()
        let unknown = ErrorBody(
            code: "ipc.outcome_unknown", category: .unavailable, message: "lost",
            retryable: true, resource: nil, fields: nil, hint: nil, details: nil,
            operationId: nil, rule: nil, items: nil)
        script.answer(.outcomeUnknown(unknown, key: "key-1"), ending: nil)
        let model = rename(script)
        model.name = "orders-2"
        _ = await model.submit(context: context)
        _ = await model.submit(context: context)

        #expect(script.sent.count == 2)
        #expect(script.sent[1].key == "key-1")
        #expect(reported.cards.first?.message == MutationFeedback.outcomeUnknown)
    }

    private static func refusal(_ code: String) -> ErrorBody {
        ErrorBody(
            code: code, category: .conflict, message: "refused", retryable: false,
            resource: nil, fields: nil, hint: nil, details: nil, operationId: nil, rule: nil,
            items: nil)
    }

    // MARK: - Ports

    @Test(
        .evidence(
            covers: ["REQ-010", "AC-055"],
            expected:
                "The sheet lists the IP endpoints as drafts and judges a changed port after the pause"
        ))
    func aChangedPortIsJudgedAfterThePause() async throws {
        let script = try script()
        let model = endpoints(script)
        await model.load()

        #expect(model.endpoints.map(\.name) == ["main"])
        #expect(model.endpoints.first?.mode == .fixed)
        #expect(model.endpoints.first?.portText == "5555")
        #expect(model.disabledReason == "Change a port first.")

        model.setPortText("5600", of: "main")
        await sleeper.waitUntilSleeping()
        sleeper.fire()
        while model.isValidating { await Task.yield() }

        #expect(sleeper.durations == [.milliseconds(300)])
        #expect(script.validatedEndpoints == [[EndpointRequest(name: "main", port: .fixed(5600))]])
        #expect(model.disabledReason == nil)
        #expect(model.impactNote == "orders restarts briefly on the new ports.")
    }

    @Test(
        .evidence(
            covers: ["AC-056"],
            expected:
                "A port that conflicts is never replaced on its own; the free port is offered and taken by a button"
        ))
    func aConflictOffersAFreePortAndNeverTakesItOnItsOwn() async throws {
        let script = try script()
        script.judgeEndpoints(
            .success(
                EndpointValidation(
                    valid: false,
                    problems: [
                        EndpointProblem(
                            endpoint: "main", code: "supervisor.endpoint_conflict",
                            suggestedPort: 5601)
                    ], changes: [], impact: .restart)))
        let model = endpoints(script)
        await model.load()
        model.setPortText("5600", of: "main")
        await sleeper.waitUntilSleeping()
        sleeper.fire()
        while model.isValidating { await Task.yield() }

        let check = try #require(model.check(forEndpoint: "main"))
        #expect(check.status == .error)
        #expect(check.detail == "Another Stackly instance uses this port.")
        #expect(check.suggestedPort == 5601)
        #expect(model.endpoints.first?.portText == "5600")
        #expect(model.disabledReason == "Fix the ports first.")

        model.usePort(5601, for: "main")
        #expect(model.endpoints.first?.portText == "5601")
    }

    @Test func aPortThatIsNotANumberIsNotSentToTheDaemon() async throws {
        let script = try script()
        let model = endpoints(script)
        await model.load()
        model.setPortText("abc", of: "main")

        #expect(model.disabledReason == EndpointDraft.invalidPortMessage)
        #expect(model.isValidating == false)
        #expect(script.validatedEndpoints.isEmpty)
    }

    @Test(
        .evidence(
            covers: ["REQ-010"],
            expected:
                "Change Ports sends the changed endpoints with the base revision and reports it"
        ))
    func changePortsSendsOnlyTheChangedEndpoints() async throws {
        let script = try script()
        script.answer(
            .completed(
                try ContractFixture.reply("frame-response-instance-change-endpoints-applied")),
            ending: try ConfigScript.operation("succeeded"))
        let model = endpoints(script)
        await model.load()
        model.setMode(.automatic, of: "main")
        await sleeper.waitUntilSleeping()
        sleeper.fire()
        while model.isValidating { await Task.yield() }
        let closes = await model.apply(context: context)

        #expect(closes)
        guard case .instanceChangeEndpoints(let params)? = script.sent.first?.call else {
            Issue.record("expected a change endpoints call")
            return
        }
        #expect(params.dryRun == false)
        #expect(params.baseRevision == 2)
        #expect(params.endpoints == [EndpointRequest(name: "main", port: .auto)])
        #expect(reported.cards.map(\.message) == ["Ports changed on orders."])
    }

    @Test func theSentenceForEachPortCodeIsPlain() {
        #expect(
            EndpointsModel.text(for: "supervisor.endpoint_in_use")
                == "Another process on this Mac uses this port.")
        #expect(
            EndpointsModel.text(for: "supervisor.endpoint_unavailable")
                == "This port can’t be used on this Mac.")
        #expect(EndpointsModel.text(for: "other") == "This port can’t be used.")
    }

    // MARK: - Where they open

    @Test(
        .evidence(
            covers: ["REQ-015"],
            expected:
                "Rename… and Edit Ports… are in the Instance menu and the More menu without a shortcut"
        ))
    func renameAndEditPortsAreBuiltCommandsWithoutAShortcut() {
        for id in [CommandID.renameInstance, .editPorts] {
            let entry = CommandMap.entry(for: id)
            #expect(entry.status == .built)
            #expect(entry.menu == .instance)
            #expect(entry.needsInput)
        }
        var opened: [InstanceEdit] = []
        let action = InstanceEditAction { opened.append($0) }
        let rename = action.renameAction(for: source)
        let ports = action.editPortsAction(for: source)
        #expect(rename.title == "Rename…")
        #expect(ports.title == "Edit Ports…")
        #expect(rename.mutates && ports.mutates)
        rename.perform()
        ports.perform()
        #expect(opened == [.renaming(source), .editPorts(source)])
    }

    @Test func theMoreMenuListsRenameAndEditPortsAfterDuplicate() {
        let copy = StacklyAction(id: "copy", title: "Copy", mutates: false, perform: {})
        let action = InstanceEditAction { _ in }
        let items = InstanceDetailPage.moreItems(
            copyConnection: copy, duplicate: nil,
            source: DuplicateSourceRef(id: source.id, name: source.name),
            rename: action.renameAction(for: source),
            editPorts: action.editPortsAction(for: source))
        #expect(items.map(\.title) == ["Copy", "Rename…", "Edit Ports…"])
    }

    @Test(
        .evidence(
            covers: ["REQ-015"],
            expected:
                "A config drift blocker is a Needs Attention item that opens the Configuration tab"
        ))
    func aConfigDriftBlockerIsAnItemThatOpensTheConfigurationTab() {
        let blocker = InstanceBlocker.configDrift(
            ConfigDriftBlocker(
                files: ["postgresql.conf", "pg_hba.conf"], observedAt: FixtureClock.minutesAgo(3)))
        let instance = InstanceSummary.fixture(
            name: "orders", lifecycle: .stopped, blocker: blocker)
        let items = AttentionRules.items(
            instances: [instance], operations: .loaded([]),
            capacity: CapacityState(attempted: true), now: FixtureClock.now,
            locale: Locale(identifier: "en_US"), timeZone: .gmt)
        let item = items.first { $0.rule == "I11" }

        #expect(item?.title == "orders’s configuration changed")
        #expect(item?.tone == .caution)
        #expect(
            item?.message
                == "postgresql.conf and 1 more was changed outside Stackly. Stackly won’t start orders until you choose Keep Revision."
        )
        #expect(item?.destination == .configuration(instance.id))
    }

    @Test func theNavigatorOpensAnInstanceOnATab() {
        let navigator = ShellNavigator()
        let id = UUID()
        navigator.openInstance(id, tab: .configuration)
        #expect(navigator.section == .instances)
        #expect(navigator.instanceRequest?.instance == id)
        #expect(navigator.instanceRequest?.tab == .configuration)
        navigator.openInstance(id)
        #expect(navigator.instanceRequest?.tab == .summary)
    }
}
