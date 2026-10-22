import Foundation
import StacklyClient
import Testing

@testable import StacklyUI

/// The entry points and the end of the New Instance sheet (spec 0020 AC-20): who opens it, on
/// which runtime, and what happens when a create ends.
struct NewInstancePresenterTests {
    @MainActor
    private func presenter() async -> (NewInstancePresenter, CreateRig) {
        let rig = CreateRig()
        await rig.load()
        return (NewInstancePresenter(model: rig.model), rig)
    }

    @Test func presentingOpensTheSheetAndReadsTheFormOfTheChosenRuntime() async {
        let (presenter, rig) = await presenter()
        presenter.present()
        #expect(presenter.isPresented)
        await settle("the form") { rig.model.phase == .editing }
    }

    @Test func presentingForASoftwareStartsOnItsNewestRuntime() async {
        let (presenter, rig) = await presenter()
        presenter.present(software: "nats")
        #expect(rig.model.runtimeID == FixtureID.uuid(kind: FixtureID.runtime, number: 5))
    }

    @Test func theToolbarAndCommandActionOpensTheSheet() async {
        let (presenter, _) = await presenter()
        let action = presenter.action.stacklyAction(title: "New Instance…")
        #expect(action.id == "newInstance")
        #expect(action.mutates)
        action.perform()
        #expect(presenter.isPresented)
    }

    @Test func closingTheSheetEndsTheModel() async {
        let (presenter, rig) = await presenter()
        presenter.present()
        await settle("the form") { rig.model.phase == .editing }
        presenter.isPresented = false
        #expect(rig.model.phase == .choosingRuntime)
        #expect(rig.model.form == nil)
    }

    @Test func aCreateThatRunsSurvivesTheSheetClosingAndComesBack() async {
        let (presenter, rig) = await presenter()
        presenter.present()
        await settle("the first preflight") { rig.model.currentPreflight != nil }
        await rig.model.create(secrets: [])
        presenter.isPresented = false
        #expect(rig.model.phase == .creating(CreateRig.operationID))

        presenter.present()
        #expect(presenter.isPresented)
        #expect(rig.model.phase == .creating(CreateRig.operationID))
        #expect(rig.service.formCalls.count == 1)
    }

    @Test func aFinishedCreateClosesTheSheetAndTellsTheShell() async {
        let (presenter, rig) = await presenter()
        var heard: [CreateOutcome] = []
        presenter.finishHandler = { heard.append($0) }
        presenter.present()
        await settle("the first preflight") { rig.model.currentPreflight != nil }
        let outcome = CreateOutcome(
            name: "orders-db", instanceID: FixtureID.uuid(kind: FixtureID.instance, number: 20),
            start: .accepted)
        presenter.finished(outcome)
        #expect(!presenter.isPresented)
        #expect(heard == [outcome])
    }

    @Test func theModelOfTheRigReportsAFinishedCreateToThePresenter() async {
        let rig = CreateRig()
        await rig.load()
        var presenter: NewInstancePresenter?
        let model = CreateInstanceModel(
            runtimes: rig.runtimeRig.runtimes, catalog: rig.runtimeRig.catalog,
            instances: rig.pageRig.instances, service: rig.service,
            send: { call, key in rig.answer(call, key) },
            updates: { _ in
                let stream = AsyncStream<OperationSnapshot> { continuation in
                    continuation.yield(
                        CreateRig.operation(
                            state: .succeeded, stage: nil,
                            instance: FixtureID.uuid(kind: FixtureID.instance, number: 20)))
                    continuation.finish()
                }
                return stream
            },
            debounce: .zero, sleep: { _ in }, announce: { _ in },
            onFinish: { presenter?.finished($0) })
        presenter = NewInstancePresenter(model: model)
        var heard: [CreateOutcome] = []
        presenter?.finishHandler = { heard.append($0) }
        presenter?.present()
        await settle("the first preflight") { model.currentPreflight != nil }
        await model.create(secrets: [])
        await settle("the outcome") { !heard.isEmpty }
        #expect(heard.first?.name == "postgresql-17")
        #expect(presenter?.isPresented == false)
    }

    @Test func theCatalogButtonIsOffWithAReasonUntilARuntimeIsInstalled() {
        let off = ProviderDetailPanel.newInstanceAction(installed: 0, present: {})
        #expect(off.disabledReason == "Install a version first.")
        #expect(
            off.availability(gate: .available)
                == .disabled(reason: "Install a version first.", showsReason: true))
        var opened = 0
        let on = ProviderDetailPanel.newInstanceAction(installed: 2, present: { opened += 1 })
        #expect(on.disabledReason == nil)
        #expect(on.availability(gate: .available) == .enabled)
        on.perform()
        #expect(opened == 1)
    }

    @Test func theShellPublishesNewInstanceOnlyWhenItHasASheet() async {
        let (presenter, _) = await presenter()
        let presentation = ShellPresentation(status: .connected(.ready))
        let with = AppShell.shellActions(
            presentation: presentation, select: { _ in }, reconnect: {},
            newInstance: presenter.action.stacklyAction(title: "New Instance…"))
        #expect(with[.newInstance]?.mutates == true)
        let without = AppShell.shellActions(
            presentation: presentation, select: { _ in }, reconnect: {})
        #expect(without[.newInstance] == nil)
    }
}
