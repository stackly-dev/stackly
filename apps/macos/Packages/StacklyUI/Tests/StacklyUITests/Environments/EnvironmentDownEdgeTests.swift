import Foundation
import StacklyClient
import StacklyTestSupport
import Synchronization
import Testing

@testable import StacklyUI

/// Edges of Down in the app (spec 0047, AC-20, AC-21), next to `EnvironmentDownTests`: every
/// keep reason, the IDs of exactly the preview the confirmation showed, a changed set that
/// sends nothing until it is confirmed again, the key of a retry, the result card only in
/// the window that sent the Down, and the reasons Down and Up… are off.
@MainActor
@Suite(.timeLimit(.minutes(1)))
struct EnvironmentDownEdgeTests {
    let rig = PageRig(environments: FixtureEnvironments.summaries)

    /// The feedback a model showed, and how many keys it made.
    @MainActor
    final class Seen {
        var feedback: [Feedback] = []
        var keys = 0
    }

    private func model(_ seen: Seen) -> EnvironmentDownModel {
        let model = EnvironmentDownModel(
            store: rig.environments,
            newKey: {
                seen.keys += 1
                return "key-\(seen.keys)"
            })
        model.report = { seen.feedback.append($0) }
        return model
    }

    private static func changed() -> ErrorBody {
        ErrorBody(
            code: "env.down_changed", category: .conflict, message: "what down would stop changed",
            retryable: false, resource: nil, fields: nil,
            hint: "review what will stop, then bring it down again", details: nil,
            operationId: nil, rule: nil, items: nil)
    }

    private static func holder(_ name: String) -> InstanceEnvironment {
        InstanceEnvironment(
            environmentId: UUID(), name: name, alias: "db", ownership: .shared, leaseActive: true)
    }

    private static func keep(
        _ name: String, demand: [DemandSourceKind], environments: [String] = []
    ) -> DownKeep {
        DownKeep(
            instance: DownInstance(
                instanceId: UUID(), name: name, alias: name, ownership: .shared,
                lifecycle: .running),
            demand: demand, environments: environments.map(holder))
    }

    /// A preview of erp-development that stops `stop` (by name) and keeps `keep`.
    private static func preview(
        stop: [String], keep: [DownKeep] = [], leaseCount: UInt64 = 1, cancelsUp: Bool = false
    ) -> EnvDownPreview {
        let base = FixtureEnvironmentDown.preview()
        return EnvDownPreview(
            environment: base.environment,
            stop: stop.map {
                DownInstance(
                    instanceId: UUID(), name: $0, alias: $0, ownership: .dedicated,
                    lifecycle: .running)
            },
            keep: keep, idle: [], blocked: [], leaseCount: leaseCount, cancelsUp: cancelsUp)
    }

    private func sentDowns() -> [EnvDownParams] {
        rig.script.environmentSent.compactMap { call in
            if case .envDown(let params) = call { return params }
            return nil
        }
    }

    // MARK: - Copy Down

    @Test(
        .evidence(
            covers: ["REQ-022", "NFR-012"],
            expected:
                "The confirmation names why each instance keeps running: used by every other environment, started by you, starts at login, and both reasons of one instance joined with and"
        ))
    func theConfirmationNamesEveryReasonAnInstanceKeepsRunning() {
        let preview = Self.preview(
            stop: ["cache"],
            keep: [
                Self.keep("pg", demand: [.environment], environments: ["reporting", "billing"]),
                Self.keep("queue", demand: [.manual]),
                Self.keep("both", demand: [.manual, .environment], environments: ["reporting"]),
                Self.keep("boot", demand: [.startup]),
                Self.keep("anon", demand: [.environment]),
            ])

        let lines = EnvironmentCopy.downMessage(preview).components(separatedBy: "\n")

        #expect(
            lines.contains(
                "Keeps running: pg (used by reporting, billing), queue (started by you), both (started by you and used by reporting), boot (starts at login), anon (used by an environment)."
            ), "\(lines)")
        #expect(lines.contains("Stops: cache."))
        #expect(
            lines.last?.hasSuffix("Data is kept.") == true || lines[0].hasSuffix("Data is kept."))
    }

    @Test func anUpToCancelAloneStillOpensTheConfirmation() async throws {
        rig.script.downPreviews = [Self.preview(stop: [], leaseCount: 0, cancelsUp: true)]
        let seen = Seen()
        let model = model(seen)

        await model.begin(environment: FixtureEnvironments.erpID)

        let request = try #require(model.confirmation)
        #expect(request.message.components(separatedBy: "\n").contains("Nothing needs to stop."))
        #expect(request.message.hasSuffix("This also cancels the Up in progress."))
        #expect(seen.feedback.isEmpty)
        #expect(sentDowns().isEmpty)
    }

    // MARK: - The IDs that go to the daemon

    @Test(
        .evidence(
            covers: ["REQ-022"],
            expected:
                "Bring Down sends env.down with exactly the stop IDs of the preview the confirmation showed, even when the daemon would answer another preview by then"
        ))
    func bringDownSendsTheIDsOfThePreviewItShowedNotAFresherOne() async throws {
        let shown = Self.preview(stop: ["cache", "web"])
        let later = Self.preview(stop: ["cache", "web", "jobs"])
        rig.script.downPreviews = [shown, later]
        let seen = Seen()
        let model = model(seen)

        await model.begin(environment: FixtureEnvironments.erpID)
        let request = try #require(model.confirmation)
        request.perform()
        for _ in 0..<50 where sentDowns().isEmpty { await Task.yield() }

        let sent = try #require(sentDowns().first)
        #expect(sent.expectedStopInstanceIds == shown.stop.map(\.instanceId))
        #expect(sent.id == FixtureEnvironments.erpID)
    }

    @Test func aChangedSetSendsNothingMoreUntilTheNewConfirmationIsConfirmed() async throws {
        let first = Self.preview(stop: ["cache"])
        let fresh = Self.preview(stop: ["cache", "jobs"])
        rig.script.downPreviews = [fresh]
        rig.script.downOutcomes = [
            .rejected(Self.changed()), .completed(.envDown(FixtureEnvironmentDown.reply())),
        ]
        let seen = Seen()
        let model = model(seen)

        await model.bringDown(first)
        let again = try #require(model.confirmation)
        #expect(again.message.hasPrefix("What Down will stop changed. Review it again."))
        for _ in 0..<20 { await Task.yield() }
        #expect(sentDowns().count == 1, "no second env.down on its own")

        again.perform()
        for _ in 0..<50 where sentDowns().count < 2 { await Task.yield() }
        let downs = sentDowns()
        #expect(downs.count == 2)
        #expect(downs[0].expectedStopInstanceIds == first.stop.map(\.instanceId))
        #expect(downs[1].expectedStopInstanceIds == fresh.stop.map(\.instanceId))
    }

    @Test func aRetryAfterAnUnknownOutcomeReusesTheKeyAndANewDownTakesANewOne() async {
        let lost = ErrorBody(
            code: "ipc.connection_lost", category: .unavailable, message: "the answer was lost",
            retryable: true, resource: nil, fields: nil, hint: nil, details: nil,
            operationId: nil, rule: nil, items: nil)
        rig.script.downOutcomes = [
            .outcomeUnknown(lost, key: "key-1"),
            .completed(.envDown(FixtureEnvironmentDown.reply())),
        ]
        let seen = Seen()
        let model = model(seen)

        await model.bringDown(FixtureEnvironmentDown.preview())
        #expect(seen.keys == 1)
        await model.bringDown(FixtureEnvironmentDown.preview())
        #expect(seen.keys == 1, "the retry reuses the key of the lost answer")
        await model.bringDown(FixtureEnvironmentDown.preview())
        #expect(seen.keys == 2, "a Down after one that completed takes a new key")
    }

    // MARK: - The result card

    @Test(
        .evidence(
            covers: ["REQ-022", "NFR-012"],
            expected:
                "Only the window that sent a Down shows its result card; another window's Down model shows nothing when that operation ends"
        ))
    func onlyTheWindowThatSentTheDownShowsItsResult() async {
        rig.script.operations = [
            FixtureEnvironmentDown.downOperation(
                state: .succeeded, done: 1, total: 1,
                result: FixtureEnvironmentDown.result(stopped: 1))
        ]
        let sender = Seen()
        let other = Seen()
        let sending = model(sender)
        let watching = model(other)
        let ended = EndedOperation(
            operation: FixtureEnvironmentDown.downID, environment: FixtureEnvironments.erpID)

        await sending.bringDown(FixtureEnvironmentDown.preview())
        await watching.ended(ended)
        await sending.ended(ended)
        await sending.ended(ended)

        #expect(other.feedback.isEmpty)
        #expect(sender.feedback.count == 1, "once, for the Down it sent")
    }

    // MARK: - Why Down and Up… are off

    @Test(
        .evidence(
            covers: ["NFR-012"],
            expected:
                "Down is off with Already bringing down while a Down runs, on while an Up runs, and off through the mutation gate while offline; Up… is off with Bringing down while a Down runs, Already bringing up while an Up runs, and through the gate while offline"
        ))
    func downAndUpAreOffWithTheRightReasonInEachState() {
        let offline = MutationAvailability.blocked(reason: "Stackly isn’t running", since: .now)
        let idle = FixtureEnvironments.summaries.first { $0.id == FixtureEnvironments.erpID }
        let erp = idle ?? FixtureEnvironments.summaries[0]
        let down = { (summary: EnvironmentSummary) in
            EnvironmentDetailPage.downAction(summary: summary, isBusy: false, perform: {})
        }
        let up = { (summary: EnvironmentSummary) in
            EnvironmentDetailPage.upAction(
                summary: summary, manifestState: .inSync, problemCount: 0, declaredName: nil,
                perform: {})
        }

        #expect(down(erp).availability(gate: .available) == .enabled)
        #expect(
            down(erp).availability(gate: offline)
                == .disabled(reason: "Stackly isn’t running", showsReason: false))
        #expect(
            down(FixtureEnvironmentDown.bringingDownSummary).availability(gate: .available)
                == .disabled(reason: "Already bringing down", showsReason: true))
        #expect(
            down(FixtureEnvironmentUp.bringingUpSummary).availability(gate: .available)
                == .enabled)
        #expect(
            EnvironmentDetailPage.downAction(summary: erp, isBusy: true, perform: {})
                .availability(gate: .available) == .busy)

        #expect(
            up(FixtureEnvironmentDown.bringingDownSummary).availability(gate: .available)
                == .disabled(reason: "Bringing down", showsReason: true))
        #expect(
            up(FixtureEnvironmentUp.bringingUpSummary).availability(gate: .available)
                == .disabled(reason: "Already bringing up", showsReason: true))
        #expect(
            up(erp).availability(gate: offline)
                == .disabled(reason: "Stackly isn’t running", showsReason: false))
    }
}

/// The lease and the override in the app (spec 0047, AC-22, AC-23), next to
/// `EnvironmentLeaseTests` and `EnvironmentOverrideTests`.
@MainActor
@Suite(.timeLimit(.minutes(1)))
struct EnvironmentLeaseEdgeTests {
    private static func entry(_ name: String, active: Bool) -> InstanceEnvironment {
        InstanceEnvironment(
            environmentId: UUID(), name: name, alias: "db", ownership: .shared,
            leaseActive: active)
    }

    @Test func theOwnershipRowSaysLeaseForASharedInstanceAndNotForAReleasedOne() throws {
        let shared = try #require(
            FixtureEnvironments.instances.first { $0.ownership == .shared },
            "a shared fixture instance")
        #expect(
            InstanceInspector.ownershipText(
                of: shared.with(environments: [Self.entry("reporting", active: true)]))
                == "Shared · lease")
        #expect(
            InstanceInspector.ownershipText(
                of: shared.with(environments: [
                    Self.entry("reporting", active: false), Self.entry("erp", active: true),
                ])) == "Shared · lease", "one active lease is enough")
        #expect(
            InstanceInspector.ownershipText(
                of: shared.with(environments: [Self.entry("reporting", active: false)]))
                == "Shared", "a released lease says nothing")
        #expect(InstanceInspector.ownershipText(of: shared.with(environments: [])) == "Shared")
    }

    /// What the scripted daemon was sent.
    @MainActor
    final class Sent {
        var calls: [(Call, String)] = []
        var confirmations: [ConfirmationRequest] = []
        var feedback: [Feedback] = []
    }

    private static func refusal() -> ErrorBody {
        ErrorBody(
            code: "supervisor.stop_requires_override", category: .conflict,
            message: "an environment holds this instance", retryable: false, resource: nil,
            fields: nil, hint: "stop it anyway with the override, or bring the environment down",
            details: nil, operationId: nil, rule: nil,
            items: [
                ErrorItem(
                    kind: "environment",
                    fields: [
                        ErrorDetail(key: "environmentId", value: UUID().uuidString),
                        ErrorDetail(key: "name", value: "reporting"),
                        ErrorDetail(key: "alias", value: "db"),
                        ErrorDetail(key: "ownership", value: "shared"),
                    ])
            ])
    }

    @Test(
        .evidence(
            covers: ["REQ-028", "BR-015"],
            expected:
                "Cancel on the override confirmation of a refused Stop sends nothing more: one instance.stop with overrideLeases false, no feedback"
        ))
    func cancellingTheOverrideConfirmationSendsNothing() async throws {
        let sent = Sent()
        let model = InstanceActionModel(
            send: { call, key in
                sent.calls.append((call, key))
                return .rejected(Self.refusal())
            },
            updates: { _ in AsyncStream { $0.finish() } },
            readOperation: { _ in nil })
        let context = InstanceActionContext(
            report: { sent.feedback.append($0) },
            confirm: { sent.confirmations.append($0) })

        await model.perform(.stop, for: FixtureEnvironments.instances[0], context: context)?
            .value
        _ = try #require(sent.confirmations.first)
        // Cancel: the confirmation is dropped without calling perform.
        sent.confirmations.removeAll()
        for _ in 0..<50 { await Task.yield() }

        #expect(sent.calls.count == 1)
        guard case .instanceStop(let params) = try #require(sent.calls.first?.0) else {
            Issue.record("expected instance.stop")
            return
        }
        #expect(params.overrideLeases == false)
        #expect(sent.feedback.isEmpty)
    }

    @Test func anotherRefusalOfAStopOpensNoOverrideConfirmation() async {
        let sent = Sent()
        let busy = ErrorBody(
            code: "resource.busy", category: .conflict, message: "busy", retryable: false,
            resource: nil, fields: nil, hint: nil, details: nil, operationId: nil, rule: nil,
            items: nil)
        let model = InstanceActionModel(
            send: { call, key in
                sent.calls.append((call, key))
                return .rejected(busy)
            },
            updates: { _ in AsyncStream { $0.finish() } },
            readOperation: { _ in nil })
        let context = InstanceActionContext(
            report: { sent.feedback.append($0) },
            confirm: { sent.confirmations.append($0) })

        await model.perform(.stop, for: FixtureEnvironments.instances[0], context: context)?
            .value

        #expect(sent.confirmations.isEmpty)
        #expect(sent.feedback.map(\.code) == ["resource.busy"])
        #expect(sent.calls.count == 1)
    }

    // MARK: - Ports sheet

    private let sleeper = ManualSleep()
    private let source = InstanceEditSource(id: ConfigScript.instance, name: "orders")

    private static func verdict(leases: [InstanceEnvironment]) -> EndpointValidation {
        EndpointValidation(
            valid: true, problems: [],
            changes: [EndpointChange(name: "main", old: .fixed(5555), new: .fixed(5600))],
            impact: .restart, leases: leases)
    }

    @Test(
        .evidence(
            covers: ["REQ-028", "BR-015"],
            expected:
                "A port change that instance.endpoints_require_override refuses, because a lease came after the dry run, is reported, judged again, and shows the caution and Change Anyway without sending again on its own; the caution names one environment"
        ))
    func aLeaseAfterTheDryRunShowsTheCautionWithoutSendingAgain() async throws {
        let script = try ConfigScript(view: ConfigScript.fakeView())
        script.judgeEndpoints(.success(Self.verdict(leases: [])))
        let sleeper = sleeper
        let model = EndpointsModel(
            source: source, service: script.service,
            sleep: { try await sleeper.sleep(for: $0) })
        await model.load()
        model.setPortText("5600", of: "main")
        await sleeper.waitUntilSleeping()
        sleeper.fire()
        while model.isValidating { await Task.yield() }
        #expect(model.primaryTitle == "Change Ports")

        let refused = ErrorBody(
            code: "instance.endpoints_require_override", category: .conflict,
            message: "an environment holds this instance", retryable: false, resource: nil,
            fields: nil,
            hint: "change the ports anyway with the override, or bring the environment down first",
            details: nil, operationId: nil, rule: nil, items: nil)
        script.answer(.rejected(refused), ending: nil)
        script.judgeEndpoints(
            .success(
                Self.verdict(leases: [
                    InstanceEnvironment(
                        environmentId: UUID(), name: "shop", alias: "db", ownership: .shared,
                        leaseActive: true)
                ])))
        var reported: [Feedback] = []
        let closes = await model.apply(
            context: InstanceActionContext(report: { reported.append($0) }))
        while model.isValidating { await Task.yield() }

        #expect(closes == false, "the sheet stays open")
        #expect(reported.map(\.code) == ["instance.endpoints_require_override"])
        #expect(
            model.leaseWarning
                == "shop uses this instance. Changing its ports can break that environment until you update what connects to it."
        )
        #expect(model.primaryTitle == "Change Anyway")
        let changes = script.sent.filter { sent in
            if case .instanceChangeEndpoints(let params) = sent.call {
                return params.dryRun == false
            }
            return false
        }
        #expect(changes.count == 1, "nothing is sent again with the override on its own")
        guard case .instanceChangeEndpoints(let params)? = changes.first?.call else {
            Issue.record("expected instance.change_endpoints")
            return
        }
        #expect(params.overrideLeases == nil)
    }
}

/// Stop All, Turn Off, and Operations with a Down (spec 0047, AC-21, AC-24): the menu bar's
/// own client sends the override with the preview and with exactly the IDs of the preview it
/// showed, Turn Off sends it with `agent_off`, and the stops of a Down nest under it.
@MainActor
@Suite(.timeLimit(.minutes(1)))
struct EnvironmentStopAllEdgeTests {
    private static let leased = UUID(uuidString: "0192f3a5-0000-7000-8000-000000000011") ?? UUID()
    private static let plain = UUID(uuidString: "0192f3a5-0000-7000-8000-000000000012") ?? UUID()

    private static func preview() -> StopAllPreview {
        StopAllPreview(
            items: [
                StopAllItem(
                    instanceId: leased, name: "shop-db", lifecycle: .running,
                    demand: [.environment],
                    autostart: InstanceAutostart(policy: .manual, session: .none),
                    environments: [
                        InstanceEnvironment(
                            environmentId: UUID(), name: "shop", alias: "db",
                            ownership: .dedicated, leaseActive: true)
                    ]),
                StopAllItem(
                    instanceId: plain, name: "cache", lifecycle: .running, demand: [.manual],
                    autostart: InstanceAutostart(policy: .manual, session: .none),
                    environments: nil),
            ],
            alreadyStopping: [], skipped: [], atLoginCount: 0,
            affectedEnvironments: [
                AffectedEnvironment(environmentId: UUID(), name: "shop", instanceCount: 1)
            ])
    }

    private func settle(_ what: String, _ done: () -> Bool) async throws {
        for _ in 0..<400 where done() == false {
            try await Task.sleep(for: .milliseconds(5))
        }
        #expect(done(), "stuck: \(what)")
    }

    @Test(
        .evidence(
            covers: ["REQ-027", "BR-015"],
            expected:
                "The menu bar's own client reads instance.preview_stop_all with overrideLeases true and sends instance.stop_all with overrideLeases true and exactly the IDs of that preview"
        ))
    func theMenuBarSendsTheOverrideAndTheIDsOfThePreviewItShowed() async throws {
        let rig = DaemonModelTests.Fixture()
        rig.link.result.withLock { $0 = .success(.instancePreviewStopAll(Self.preview())) }
        let model = MenuBarModel(daemon: rig.model, connected: { true })

        model.beginStopAll()
        try await settle("the preview") {
            if case .preview = model.phase { true } else { false }
        }
        rig.link.result.withLock {
            $0 = .success(
                .instanceStopAll(
                    StopAllResult(stopped: [Self.leased, Self.plain], failed: [], skipped: [])))
        }
        model.confirmStopAll()
        try await settle("the result") {
            if case .result = model.phase { true } else { false }
        }

        let calls = rig.link.calls.withLock { $0.map(\.0) }
        let previews = calls.compactMap { call -> StopAllPreviewParams? in
            if case .instancePreviewStopAll(let params) = call { return params }
            return nil
        }
        let stops = calls.compactMap { call -> StopAllParams? in
            if case .instanceStopAll(let params) = call { return params }
            return nil
        }
        #expect(previews.count == 1)
        #expect(previews.first?.overrideLeases == true)
        #expect(stops.count == 1)
        #expect(stops.first?.expectedInstanceIds == [Self.leased, Self.plain])
        #expect(stops.first?.overrideLeases == true)
    }

    @Test(
        .evidence(
            covers: ["BR-015"],
            expected:
                "Turn Off reads the preview with overrideLeases true, lists the instance an environment holds, and sends daemon.shutdown agent_off with overrideLeases true"
        ))
    func turnOffSendsTheOverrideWithThePreviewAndAgentOff() async throws {
        let rig = DaemonModelTests.Fixture()
        rig.link.result.withLock { $0 = .success(.instancePreviewStopAll(Self.preview())) }
        let turnOff = rig.model.turnOff

        await turnOff.begin()
        let plan = try #require(turnOff.plan)
        #expect(plan.canTurnOff)
        #expect(plan.message.contains("shop-db · Used by shop"))
        let pending = ErrorBody(
            code: "daemon.restart_pending", category: .conflict, message: "a restart is pending",
            retryable: false, resource: nil, fields: nil, hint: nil, details: nil,
            operationId: nil, rule: nil, items: nil)
        rig.link.result.withLock { $0 = .failure(.rejected(pending)) }
        await turnOff.confirm()

        let calls = rig.link.calls.withLock { $0.map(\.0) }
        let preview = calls.compactMap { call -> StopAllPreviewParams? in
            if case .instancePreviewStopAll(let params) = call { return params }
            return nil
        }
        let shutdown = calls.compactMap { call -> ShutdownParams? in
            if case .daemonShutdown(let params) = call { return params }
            return nil
        }
        #expect(preview.first?.overrideLeases == true)
        #expect(shutdown.count == 1)
        #expect(shutdown.first?.mode == .agentOff)
        #expect(shutdown.first?.overrideLeases == true)
    }

    @Test(
        .evidence(
            covers: ["NFR-012"],
            expected:
                "In Operations the stops of a Down follow it, indented, the Down reads Partial, and a stop whose Down is not in the list shows on its own"
        ))
    func theStopsOfADownNestUnderItAndAnOrphanShowsOnItsOwn() {
        let operations = FixtureEnvironmentDown.incompleteOperations
        let rows = OperationNesting.rows(operations)

        #expect(rows.map(\.id) == operations.map(\.id))
        #expect(rows.map(\.isChild) == [false, true, true])
        #expect(StatusAdapter.appearance(of: rows[0].operation).label == "Partial")

        let orphans = OperationNesting.rows(Array(operations.dropFirst()))
        #expect(orphans.allSatisfy { $0.isChild == false })
        #expect(orphans.count == 2)
    }
}
