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
            debounce: .zero, sleep: { _ in },
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

    // covers: spec 0037 review, Major 2
    @Test func theCatalogPanelOfAProviderAsksForTheFormOfThatProvider() async {
        let (presenter, rig) = await presenter()
        presenter.present(software: "nats", provider: "acme/nats")
        await settle("the form") { rig.model.phase == .editing }
        #expect(rig.service.formProviders == ["acme/nats"])

        presenter.isPresented = false
        presenter.present()
        await settle("the second form") { rig.model.phase == .editing }
        #expect(rig.service.formProviders == ["acme/nats", nil])
    }

    // covers: spec 0037 review, Major 2
    @Test func aRuntimeOfAnotherSoftwareDropsTheRequestedProvider() async {
        let (presenter, rig) = await presenter()
        presenter.present(software: "nats", provider: "acme/nats")
        await settle("the form") { rig.model.phase == .editing }
        let other = rig.model.installedRuntimes.first { $0.software != "nats" }
        rig.model.selectRuntime(other?.id)
        await settle("the other form") { rig.service.formProviders.count == 2 }
        #expect(rig.service.formProviders.last == .some(nil))
    }

    // covers: spec 0037 review, Major 2
    @Test func anAmbiguousProviderOffersItsCandidatesAndChoosingOneReadsItsForm() async {
        let (presenter, rig) = await presenter()
        var body = ErrorBody.create(
            "instance.provider_ambiguous",
            "no single approved provider runs this software; choose one of the candidates")
        body = ErrorBody(
            code: body.code, category: body.category, message: body.message,
            retryable: false, resource: nil, fields: nil, hint: nil,
            details: [ErrorDetail(key: "candidates", value: "acme/nats,beta/nats")],
            operationId: nil, rule: nil, items: nil)
        let refusal = body
        let service = rig.service
        rig.service.onForm = { _ throws(ClientError) in
            if service.formProviders.last == .some(nil) { throw .rejected(refusal) }
            return FixtureCreateForm.database
        }
        presenter.present(software: "nats")
        await settle("the refusal") {
            if case .formFailed = rig.model.phase { return true }
            return false
        }
        #expect(rig.model.providerCandidates == ["acme/nats", "beta/nats"])
        if case .formFailed(let failure) = rig.model.phase {
            #expect(failure.code == "instance.provider_ambiguous")
            #expect(failure.message.contains("choose one of the candidates"))
        }

        rig.model.chooseProvider("beta/nats")
        await settle("the chosen form") { rig.model.phase == .editing }
        #expect(rig.service.formProviders == [nil, "beta/nats"])
        #expect(rig.model.providerCandidates.isEmpty)
    }

    // covers: spec 0037 Layar (provider choice), review Major 2
    @Test func theCandidateRowsKeepTheDaemonsOrderAndTakeNamesFromTheSnapshot() async throws {
        let (presenter, rig) = await presenter()
        let known = try #require(rig.runtimeRig.catalog.providers?.first)
        let unknown = "zeta/unknown"
        let refusal = ErrorBody(
            code: "instance.provider_ambiguous", category: .conflict,
            message: "no single approved provider runs this software; choose one of the candidates",
            retryable: false, resource: nil, fields: nil, hint: nil,
            details: [ErrorDetail(key: "candidates", value: "\(unknown),\(known.provider)")],
            operationId: nil, rule: nil, items: nil)
        rig.service.onForm = { _ throws(ClientError) in throw .rejected(refusal) }
        presenter.present()
        await settle("the refusal") { !rig.model.providerCandidates.isEmpty }

        let rows = rig.model.providerCandidateRows
        #expect(rows.map(\.id) == [unknown, known.provider])
        #expect(rows[0].displayName == nil)
        #expect(rows[0].trustLabel == nil)
        #expect(rows[1].displayName == known.displayName)
    }

    // covers: spec 0037 Layar (provider choice), review Major 2
    @Test func aTrustRefusalForAChosenCandidateShowsTheDaemonsMessageAndHint() async {
        let (presenter, rig) = await presenter()
        let ambiguous = ErrorBody(
            code: "instance.provider_ambiguous", category: .conflict,
            message: "no single approved provider runs this software; choose one of the candidates",
            retryable: false, resource: nil, fields: nil, hint: nil,
            details: [ErrorDetail(key: "candidates", value: "acme/nats")],
            operationId: nil, rule: nil, items: nil)
        let pending = ErrorBody(
            code: "provider.approval_required", category: .conflict,
            message: "provider acme/nats is not approved", retryable: false, resource: nil,
            fields: nil, hint: "approve it with stackly provider approve acme/nats",
            details: nil, operationId: nil, rule: nil, items: nil)
        let service = rig.service
        rig.service.onForm = { _ throws(ClientError) in
            throw .rejected(service.formProviders.last == .some(nil) ? ambiguous : pending)
        }
        presenter.present(software: "nats")
        await settle("the choice") { !rig.model.providerCandidates.isEmpty }

        rig.model.chooseProvider("acme/nats")
        await settle("the trust refusal") {
            if case .formFailed(let failure) = rig.model.phase {
                return failure.code == "provider.approval_required"
            }
            return false
        }
        #expect(rig.model.providerCandidates.isEmpty)
        if case .formFailed(let failure) = rig.model.phase {
            #expect(failure.message.contains("is not approved"))
            #expect(failure.message.contains("stackly provider approve acme/nats"))
        }
    }
}
