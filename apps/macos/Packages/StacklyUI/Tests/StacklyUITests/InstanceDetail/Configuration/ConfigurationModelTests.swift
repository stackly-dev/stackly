import Foundation
import StacklyClient
import StacklyTestSupport
import Testing

@testable import StacklyUI

/// `ConfigurationModel` (spec 0029, *GUI*; AC-12, AC-15, AC-16, AC-18): the locked rows built
/// from the schema, the debounced validation, the label of Apply per impact, the banners'
/// conditions, and the mutations, with a scripted daemon.
@MainActor
@Suite(.timeLimit(.minutes(1)))
struct ConfigurationModelTests {
    private let sleeper = ManualSleep()
    private let reported = Reported()

    @MainActor
    final class Reported {
        var cards: [Feedback] = []
    }

    private func model(_ script: ConfigScript) -> ConfigurationModel {
        let sleeper = sleeper
        return ConfigurationModel(
            instanceId: ConfigScript.instance, service: script.service,
            sleep: { try await sleeper.sleep(for: $0) })
    }

    private var context: InstanceActionContext {
        let reported = reported
        return InstanceActionContext(report: { reported.cards.append($0) })
    }

    private func opened(_ view: InstanceConfigView? = nil) async throws -> (
        ConfigurationModel, ConfigScript
    ) {
        let script = try ConfigScript(view: view ?? ConfigScript.fakeView())
        let model = model(script)
        await model.open()
        return (model, script)
    }

    /// Edits `workers` and lets the debounce pass and the verdict arrive.
    private func edit(_ model: ConfigurationModel, workers: String) async {
        model.schema?.setText("workers", workers)
        await sleeper.waitUntilSleeping()
        sleeper.fire()
        while model.isValidating { await Task.yield() }
    }

    // MARK: - Form

    @Test(
        .evidence(
            covers: ["VAL-012", "REQ-015"],
            expected:
                "A set once field, a secret, and a managed field are locked rows; an editable field says when it applies and its default"
        ))
    func lockedFieldsAreReadOnlyAndEditableOnesSayWhenTheyApply() async throws {
        let (model, _) = try await opened()
        let schema = try #require(model.schema)

        #expect(model.phase == .loaded)
        #expect(schema.locks["dataSeed"] == .setOnce)
        #expect(schema.locks["adminToken"] == .secret(reference: "secret:fake-orders/adminToken"))
        #expect(schema.locks["greeting"] == nil)
        #expect(schema.notes["greeting"] == "Applies on reload · default hello")
        #expect(schema.notes["workers"] == "Requires restart · default 2")
        #expect(schema.text("workers") == "4")
        #expect(model.hasPending == false)
        #expect(model.impactTag == "In sync")
        #expect(model.applyDisabledReason == "No changes to apply.")
    }

    @Test func aProviderWithoutReloadSaysRestartForAReloadField() async throws {
        let view = try ConfigScript.fakeView()
        let noReload = InstanceConfigView(
            revision: view.revision, lastKnownGood: view.lastKnownGood, fields: view.fields,
            endpoints: view.endpoints, history: view.history, effectiveFiles: view.effectiveFiles,
            effectiveFilesNote: nil, drift: nil, reloadSupported: false)
        let (model, _) = try await opened(noReload)
        #expect(model.schema?.notes["greeting"] == "Requires restart · default hello")
    }

    // MARK: - Validation

    @Test(
        .evidence(
            covers: ["AC-015"],
            expected:
                "An edit is judged once after 300 ms with the active revision as base and the secret reference untouched"
        ))
    func anEditIsJudgedAfterTheDebounceWithTheActiveSettingsUnderIt() async throws {
        let (model, script) = try await opened()
        await edit(model, workers: "8")

        #expect(sleeper.durations == [.milliseconds(300)])
        let change = try #require(script.validated.first)
        guard case .settings(let settings) = change else {
            Issue.record("expected a settings change")
            return
        }
        #expect(settings.members["workers"] == .number(8))
        #expect(settings.members["dataSeed"] == .string("seed"))
        #expect(settings.members["adminToken"] == .string("secret:fake-orders/adminToken"))
        #expect(model.impact == .restart)
        #expect(model.impactTag == "Requires restart")
        #expect(model.applyDisabledReason == nil)
    }

    @Test func editingBackToTheActiveValueLeavesNothingPending() async throws {
        let (model, script) = try await opened()
        await edit(model, workers: "8")
        model.schema?.setText("workers", "4")

        #expect(model.hasPending == false)
        #expect(model.validation == nil)
        #expect(model.applyDisabledReason == "No changes to apply.")
        #expect(script.validated.count == 1)
    }

    @Test(
        .evidence(
            covers: ["AC-015"],
            expected: "The field errors of the verdict show under their fields and Apply is off"
        ))
    func anInvalidVerdictShowsItsErrorsAndKeepsApplyOff() async throws {
        let (model, script) = try await opened()
        script.judge(.success(try ConfigScript.verdict("invalid")))
        await edit(model, workers: "999")

        #expect(model.schema?.errorTexts(for: "workers") == ["Use a whole number from 1 to 64."])
        #expect(
            model.schema?.errorTexts(for: "dataSeed") == [
                "Can’t be changed after the instance is created."
            ])
        #expect(model.applyDisabledReason == "Fix the errors in the form first.")
    }

    @Test func aRevisionConflictFromTheVerdictKeepsThePendingChange() async throws {
        let (model, script) = try await opened()
        let body = try ContractFixture.error("frame-error-config-revision-conflict")
        script.judge(.failure(.rejected(body)))
        await edit(model, workers: "8")

        #expect(model.conflict != nil)
        #expect(model.hasPending)
        #expect(model.applyDisabledReason == "This configuration changed. Reload first.")
    }

    // MARK: - Buttons

    @Test(
        .evidence(
            covers: ["AC-015"],
            expected:
                "Apply says Reload, Restart…, or only Apply by the impact, and a restart confirms"
        ))
    func theApplyButtonFollowsTheImpact() async throws {
        let (model, script) = try await opened()
        func title() -> String { model.applyAction(name: "orders", context: context).title }

        script.judge(.success(try ConfigScript.verdict("restart")))
        await edit(model, workers: "8")
        #expect(title() == "Apply and Restart…")
        let restart = model.applyAction(name: "orders", context: context)
        #expect(restart.confirmation?.title == "Apply and restart “orders”?")

        let reload = ConfigValidation(
            valid: true, fieldErrors: [],
            changes: [SettingChange(key: "greeting", old: "hello", new: "hi")],
            endpointChanges: [], impact: .reload, preview: [])
        script.judge(.success(reload))
        await edit(model, workers: "9")
        #expect(title() == "Apply and Reload")
        #expect(model.applyAction(name: "orders", context: context).confirmation == nil)

        let saved = ConfigValidation(
            valid: true, fieldErrors: [], changes: reload.changes, endpointChanges: [],
            impact: .nextStart, preview: [])
        script.judge(.success(saved))
        await edit(model, workers: "10")
        #expect(title() == "Apply")
    }

    @Test func revertDropsThePendingChange() async throws {
        let (model, _) = try await opened()
        await edit(model, workers: "8")
        model.revert()

        #expect(model.hasPending == false)
        #expect(model.schema?.text("workers") == "4")
        #expect(model.revertAction.disabledReason == "No pending changes.")
    }

    // MARK: - Apply

    @Test(
        .evidence(
            covers: ["REQ-015"],
            expected: "Apply sends the change with the base revision and reports the new revision"
        ))
    func applySendsTheChangeAndReportsTheRevision() async throws {
        let (model, script) = try await opened()
        script.answer(
            .completed(try ContractFixture.reply("frame-response-instance-config-apply")),
            ending: try ConfigScript.operation("succeeded"))
        await edit(model, workers: "8")
        await model.apply(name: "orders", context: context)

        let call = try #require(script.sent.first?.call)
        guard case .instanceConfigApply(let params) = call else {
            Issue.record("expected an apply call")
            return
        }
        #expect(params.instanceId == ConfigScript.instance)
        #expect(params.baseRevision == 2)
        #expect(reported.cards.map(\.message) == ["Revision 3 applied to orders."])
        #expect(model.busy == nil)
        #expect(model.hasPending == false)
    }

    @Test func aFailedOperationNamesTheRevisionThatStaysActive() async throws {
        let (model, script) = try await opened()
        script.answer(
            .completed(try ContractFixture.reply("frame-response-instance-config-apply")),
            ending: try ConfigScript.operation("failed"))
        await edit(model, workers: "8")
        await model.apply(name: "orders", context: context)

        let card = try #require(reported.cards.first)
        #expect(card.tone == .critical)
        #expect(card.message == "Couldn’t apply the configuration to orders.")
        #expect(card.detail?.contains("Revision 3 stays active.") == true)
    }

    @Test func aRefusedApplyForAStaleRevisionShowsTheChangedBanner() async throws {
        let (model, script) = try await opened()
        let body = try ContractFixture.error("frame-error-config-revision-conflict")
        script.answer(.rejected(body), ending: nil)
        await edit(model, workers: "8")
        await model.apply(name: "orders", context: context)

        #expect(model.conflict != nil)
        #expect(model.hasPending)
    }

    // MARK: - History and banners

    @Test(
        .evidence(
            covers: ["REQ-015"],
            expected:
                "Restore loads an older revision as a pending change and applies it as a restore"
        ))
    func restoreLoadsAnOlderRevisionAsAPendingChange() async throws {
        let view = try ConfigScript.fakeView()
        let older = view.revision.with(
            number: 1, readyAt: .some(nil), isActive: false, isLastKnownGood: false,
            values: JSONObject([
                "greeting": .string("hey"), "workers": .number(2), "dataSeed": .string("seed"),
                "adminToken": .string("secret:fake-orders/adminToken"),
            ]))
        let (model, script) = try await opened(view.with(history: [view.revision, older]))
        model.restore(older)

        #expect(model.schema?.text("greeting") == "hey")
        #expect(model.hasPending)
        #expect(model.pendingChange == .restore(1))
        await sleeper.waitUntilSleeping()
        sleeper.fire()
        while model.isValidating { await Task.yield() }
        #expect(script.validated == [.restore(1)])
    }

    @Test func anEditedRestoreIsSentAsSettings() async throws {
        let view = try ConfigScript.fakeView()
        let older = view.revision.with(number: 1, isActive: false, isLastKnownGood: false)
        let (model, _) = try await opened(view.with(history: [view.revision, older]))
        model.restore(older)
        model.schema?.setText("workers", "7")

        guard case .settings? = model.pendingChange else {
            Issue.record("expected a settings change")
            return
        }
    }

    @Test(
        .evidence(
            covers: ["AC-015"],
            expected: "The roll back banner shows while the active revision never reached ready"
        ))
    func theRollBackTargetIsTheLastKnownGoodWhenTheActiveRevisionNeverWasReady() async throws {
        let view = try ConfigScript.fakeView()
        let (ready, _) = try await opened()
        #expect(ready.rollBackTarget == nil)
        #expect(ready.rollBackAction(name: "orders", context: context) == nil)

        let broken = view.revision.with(number: 3, readyAt: .some(nil), isLastKnownGood: false)
        let (model, _) = try await opened(view.with(revision: broken, lastKnownGood: .some(2)))
        #expect(model.rollBackTarget == 2)
        let action = try #require(model.rollBackAction(name: "orders", context: context))
        #expect(action.title == "Roll Back to Revision 2…")
        #expect(action.confirmation?.title == "Roll back “orders” to revision 2?")
    }

    @Test func rollBackSendsTheRollbackChange() async throws {
        let view = try ConfigScript.fakeView()
        let broken = view.revision.with(number: 3, readyAt: .some(nil), isLastKnownGood: false)
        let (model, script) = try await opened(view.with(revision: broken, lastKnownGood: .some(2)))
        script.answer(
            .completed(try ContractFixture.reply("frame-response-instance-config-apply")),
            ending: try ConfigScript.operation("succeeded"))
        await model.rollBack(name: "orders", context: context)

        guard case .instanceConfigApply(let params)? = script.sent.first?.call else {
            Issue.record("expected an apply call")
            return
        }
        #expect(params.change == .rollback)
        #expect(params.baseRevision == 3)
    }

    @Test(
        .evidence(
            covers: ["REQ-015"],
            expected: "Drift turns Apply off and Keep Revision asks with Cancel as the default"
        ))
    func driftBlocksApplyUntilKeepRevision() async throws {
        let drift = try {
            guard
                case .instanceConfigGet(let view) = try ContractFixture.reply(
                    "frame-response-instance-config-get-drift"),
                let drift = view.drift
            else { throw ContractFixture.Mismatch(description: "no drift") }
            return drift
        }()
        let (model, script) = try await opened(
            try ConfigScript.fakeView().with(drift: .some(drift)))
        await edit(model, workers: "8")

        #expect(
            model.applyDisabledReason
                == "Configuration files changed outside Stackly. Choose Keep Revision first.")
        let keep = try #require(model.keepRevisionAction(name: "orders", context: context))
        #expect(keep.title == "Keep Revision 2…")
        #expect(keep.role.isDestructive)

        script.answer(
            .completed(try ContractFixture.reply("frame-response-instance-config-keep-revision")),
            ending: try ConfigScript.operation("succeeded"))
        await model.keepRevision(name: "orders", context: context)
        guard case .instanceConfigKeepRevision? = script.sent.first?.call else {
            Issue.record("expected a keep revision call")
            return
        }
        #expect(reported.cards.first?.message == "Configuration files restored from revision 2.")
    }

    // MARK: - Refresh

    @Test func aNewRevisionWithAPendingChangeKeepsTheChangeAndRaisesTheBanner() async throws {
        let (model, script) = try await opened()
        await edit(model, workers: "8")
        let view = try ConfigScript.fakeView()
        script.show(view.with(revision: view.revision.with(number: 3)))
        await model.refresh()

        #expect(model.conflict == 3)
        #expect(model.hasPending)
        #expect(model.activeRevision == 2)
    }

    @Test func aNewRevisionWithoutAPendingChangeLoadsAtOnce() async throws {
        let (model, script) = try await opened()
        let view = try ConfigScript.fakeView()
        script.show(view.with(revision: view.revision.with(number: 3)))
        await model.refresh()

        #expect(model.activeRevision == 3)
        #expect(model.conflict == nil)
    }
}
