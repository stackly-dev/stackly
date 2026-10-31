import Foundation
import StacklyClient
import Testing

@testable import StacklyUI

/// The state machine of the New Instance sheet over a scripted service (spec 0020 AC-20):
/// the form, the draft generation, the preflight, the Create gate, and the operation.
struct CreateInstanceModelTests {
    private static let database = FixtureID.uuid(kind: FixtureID.runtime, number: 1)
    private static let mysql = FixtureID.uuid(kind: FixtureID.runtime, number: 4)

    @MainActor
    private func opened(
        runtimes: RuntimeListPage = FixtureRuntime.page, software: String? = nil
    ) async -> CreateRig {
        let rig = CreateRig(runtimes: runtimes)
        await rig.load()
        rig.model.begin(software: software)
        return rig
    }

    @MainActor
    private func editing(
        software: String? = nil, prepare: (CreateRig) -> Void = { _ in }
    ) async -> CreateRig {
        let rig = CreateRig()
        prepare(rig)
        await rig.load()
        rig.model.begin(software: software)
        await settle("the first preflight") { rig.model.currentPreflight != nil }
        return rig
    }

    // MARK: - Opening

    @Test func openingChoosesTheFirstRuntimeAndReadsItsForm() async {
        let rig = await editing()
        let model = rig.model
        #expect(model.phase == .editing)
        #expect(rig.service.formCalls == [Self.mysql])
        #expect(model.name == "postgresql-17")
        #expect(model.endpoints.map(\.name) == ["postgres", "admin"])
        #expect(model.endpoints.allSatisfy { $0.mode == .fixed })
        #expect(model.endpoints.map(\.portText) == ["5432", "5444"])
        #expect(model.currentPreflight == FixtureCreateForm.ready)
        #expect(model.schema?.rows.count == 5)
    }

    @Test func openingFromACatalogEntryChoosesTheNewestRuntimeOfThatSoftware() async {
        let rig = await editing(software: "postgresql")
        #expect(rig.model.runtimeID == Self.database)
        #expect(rig.model.tile?.mark == "Pg")
    }

    @Test func thePickerLabelsAnExternalRuntime() {
        let runtime = FixtureRuntime.summaries[0]
        #expect(CreateInstanceModel.label(of: runtime) == "PostgreSQL 17.6")
    }

    @Test func theCategoryFilterNarrowsThePickerAndEmptiesIt() async {
        let rig = await editing()
        let model = rig.model
        model.selectCategory(.messaging)
        #expect(model.visibleRuntimes.map(\.software) == ["nats"])
        #expect(model.runtimeID == FixtureID.uuid(kind: FixtureID.runtime, number: 5))
        model.selectCategory(.cache)
        #expect(model.visibleRuntimes.isEmpty)
        #expect(model.runtimeID == nil)
        #expect(model.phase == .choosingRuntime)
        #expect(model.createDisabledReason == "Choose a runtime.")
    }

    @Test func withNoInstalledRuntimeTheSheetAsksForOne() async {
        let rig = await opened(runtimes: RuntimeListPage(items: [], next: nil, installing: []))
        #expect(rig.model.hasNoInstalledRuntimes)
        #expect(rig.model.phase == .choosingRuntime)
        #expect(rig.model.createDisabledReason == "Install a runtime first.")
    }

    @Test func aFormThatCannotBeReadFailsTheSheetAndRetryReadsItAgain() async {
        let rig = CreateRig()
        rig.service.onForm = { _ throws(ClientError) in
            throw .rejected(.create("catalog.unavailable", "the catalog is not loaded"))
        }
        await rig.load()
        rig.model.begin()
        await settle("the failure") {
            if case .formFailed = rig.model.phase { true } else { false }
        }
        #expect(rig.model.createDisabledReason == "Couldn’t load the form.")
        rig.service.onForm = { _ in FixtureCreateForm.database }
        rig.model.retryLoad()
        await settle("the form") { rig.model.phase == .editing }
        #expect(rig.service.formCalls.count == 2)
    }

    // MARK: - Generations

    @Test func aReplyForAnOlderDraftIsDropped() async {
        let gate = Gate<CreatePreflight>()
        let rig = CreateRig()
        rig.service.onPreflight = { _ in await gate.wait() }
        await rig.load()
        rig.model.begin()
        await settle("the first preflight to wait") { gate.waiting == 1 }
        rig.model.setName("orders-db")
        await settle("the second preflight to wait") { gate.waiting == 2 }

        gate.release(FixtureCreate.portConflict)
        await settle("the first reply to be handled") { rig.service.preflightCalls.count == 2 }
        await Task.yield()
        #expect(rig.model.currentPreflight == nil)
        #expect(rig.model.createDisabledReason == "Checking…")

        gate.release(FixtureCreateForm.ready)
        await settle("the current reply") { rig.model.currentPreflight != nil }
        #expect(rig.model.currentPreflight == FixtureCreateForm.ready)
        #expect(rig.service.preflightCalls.map(\.name) == ["postgresql-17", "orders-db"])
    }

    @Test func everyChangeOfTheDraftAsksAgain() async {
        let rig = await editing()
        let before = rig.service.preflightCalls.count
        rig.model.setPortText("5440", of: "postgres")
        await settle("a preflight") { rig.service.preflightCalls.count == before + 1 }
        rig.model.setStartAfterCreating(false)
        await settle("a preflight") { rig.service.preflightCalls.count == before + 2 }
        rig.model.schema?.setText("logStatement", "ddl")
        await settle("a preflight") { rig.service.preflightCalls.count == before + 3 }
        let draft = rig.service.preflightCalls.last
        #expect(draft?.start == false)
        #expect(draft?.endpoints?.first == EndpointRequest(name: "postgres", port: .fixed(5440)))
        #expect(draft?.settings.members["logStatement"] == .string("ddl"))
        #expect(draft?.provider == ProviderSelector(name: "official/postgresql", revision: nil))
    }

    @Test func aTextThatIsNotAPortIsNotSentAndKeepsCreateOff() async {
        let rig = await editing()
        rig.model.setPortText("abc", of: "postgres")
        await settle("a preflight") { rig.service.preflightCalls.count == 2 }
        #expect(rig.service.preflightCalls.last?.endpoints?.map(\.name) == ["admin"])
        #expect(rig.model.hasClientError)
        #expect(rig.model.endpoints[0].hasInvalidPort)
        #expect(rig.model.createDisabledReason == "Fix the errors above.")
        rig.model.setPortText("70000", of: "postgres")
        #expect(rig.model.endpoints[0].hasInvalidPort)
        rig.model.setPortText("0", of: "postgres")
        #expect(
            rig.model.endpoints[0].request == EndpointRequest(name: "postgres", port: .fixed(0)))
    }

    @Test func theSuggestedPortIsUsedOnlyWhenThePersonAsks() async {
        let rig = await editing { $0.service.onPreflight = { _ in FixtureCreate.portConflict } }
        let model = rig.model
        let row = model.check(forEndpoint: "postgres")
        #expect(row?.suggestedPort == FixtureCreate.suggestedPort)
        #expect(model.endpoints[0].portText == "5432")
        #expect(model.createDisabledReason == "Fix the errors above.")
        model.usePort(FixtureCreate.suggestedPort, for: "postgres")
        #expect(model.endpoints[0].mode == .fixed)
        #expect(model.endpoints[0].portText == "5433")
    }

    @Test func automaticAskForNoPort() async {
        let rig = await editing()
        rig.model.setMode(.automatic, of: "admin")
        await settle("a preflight") { rig.service.preflightCalls.count == 2 }
        #expect(
            rig.service.preflightCalls.last?.endpoints?.last
                == EndpointRequest(name: "admin", port: .auto))
    }

    @Test func theNatsFormHasItsClientAndMonitoringRowsWithFixedOrAutomaticPorts() async throws {
        let form = try contractForm("nats")
        let rig = await editing { $0.service.onForm = { _ in form } }
        let model = rig.model
        #expect(model.endpoints.map(\.name) == ["client", "monitoring"])
        #expect(model.endpoints.map(\.purpose) == [.client, .monitoring])
        #expect(model.endpoints.map(\.host) == ["127.0.0.1", "127.0.0.1"])
        #expect(model.endpoints.map(\.mode) == [.fixed, .fixed])
        #expect(model.endpoints.map(\.portText) == ["4222", "8222"])
        #expect(
            model.endpoints.compactMap(\.request) == [
                EndpointRequest(name: "client", port: .fixed(4222)),
                EndpointRequest(name: "monitoring", port: .fixed(8222)),
            ])
        model.setMode(.automatic, of: "client")
        model.setMode(.automatic, of: "monitoring")
        #expect(
            model.endpoints.compactMap(\.request) == [
                EndpointRequest(name: "client", port: .auto),
                EndpointRequest(name: "monitoring", port: .auto),
            ])
    }

    // MARK: - Create gate

    @Test func theGateGivesTheFirstReasonThatApplies() async {
        let rig = await editing()
        #expect(rig.model.createDisabledReason == nil)

        rig.service.onPreflight = { _ throws(ClientError) in
            throw .rejected(.create("daemon.starting"))
        }
        rig.model.setName("orders-db")
        await settle("the failure") { rig.model.preflightFailure != nil }
        #expect(rig.model.createDisabledReason == "Run preflight again to continue.")

        rig.service.onPreflight = { _ in FixtureCreate.portConflict }
        rig.model.retryPreflight()
        await settle("the conflict") { rig.model.preflightHasError }
        #expect(rig.model.createDisabledReason == "Fix the errors above.")
    }

    @Test func aDraftThatChangedIsCheckingUntilItsReplyArrives() async {
        let gate = Gate<CreatePreflight>()
        let rig = await editing()
        rig.service.onPreflight = { _ in await gate.wait() }
        rig.model.setName("orders-db")
        await settle("the preflight to wait") { gate.waiting == 1 }
        #expect(rig.model.createDisabledReason == "Checking…")
        gate.release(FixtureCreateForm.ready)
        await settle("the reply") { rig.model.createDisabledReason == nil }
    }

    // MARK: - Copy from

    @Test func copyFromKeepsPresentAndUnknownReferencesAndDropsTheRest() async {
        let rig = await editing()
        await settle("the list") { !rig.model.copyFromOptions.isEmpty }
        let kept = rig.model.copyFromOptions
        #expect(kept == [FixtureCreateForm.copyFromReference, "local:user/shared-token"])
        #expect(rig.model.schema?.copyFromOptions == kept)

        let rows = FixtureCreateForm.secrets.map { row in
            SecretRefSummary(
                reference: row.reference, ownerKind: row.ownerKind, ownerId: row.ownerId,
                purpose: row.purpose,
                status: row.reference.hasSuffix("token") ? .unknown : .missing,
                statusObservedAt: nil, valueOrigin: nil, valueUpdatedAt: nil, usedBy: nil,
                revision: 1)
        }
        rig.service.onSecrets = { rows }
        rig.model.retryLoad()
        await settle("the second list") { rig.service.listCalls == 2 }
        await settle("the form") { rig.model.phase == .editing }
        await settle("the options") { rig.model.copyFromOptions == ["local:user/shared-token"] }
    }

    @Test func copyFromLabelsOnlyTheReferencesOfKnownInstances() async {
        let rig = await editing()
        await settle("the list") { !rig.model.copyFromOptions.isEmpty }
        #expect(
            rig.model.copyFromLabels == [
                FixtureCreateForm.copyFromReference: "Superuser password of “pg17-dev”"
            ])
    }

    @Test func theFormIsEditableBeforeTheListComesBack() async {
        let gate = Gate<[SecretRefSummary]>()
        let rig = CreateRig()
        rig.service.onSecrets = { await gate.wait() }
        await rig.load()
        rig.model.begin()
        await settle("editing") { rig.model.phase == .editing }
        #expect(rig.model.copyFromOptions.isEmpty)
        await settle("a preflight") { rig.model.currentPreflight != nil }
        #expect(rig.model.createDisabledReason == nil)
        gate.release(FixtureCreateForm.secrets)
        await settle("the list") { !rig.model.copyFromOptions.isEmpty }
        #expect(rig.model.schema?.copyFromOptions == rig.model.copyFromOptions)
    }

    @Test func aListThatCannotBeReadHidesCopyFromAndHoldsNothingBack() async {
        let rig = await editing { rig in
            rig.service.onSecrets = { () throws(ClientError) -> [SecretRefSummary] in
                throw .rejected(.create("secret.store_busy"))
            }
        }
        #expect(rig.model.copyFromOptions.isEmpty)
        #expect(rig.model.banner == nil)
        #expect(rig.model.createDisabledReason == nil)
    }

    @Test func aReplyOfAnOlderLoadIsDropped() async {
        let forms = Gate<CreateForm>()
        let lists = Gate<[SecretRefSummary]>()
        let rig = CreateRig()
        rig.service.onForm = { _ in await forms.wait() }
        rig.service.onSecrets = { await lists.wait() }
        await rig.load()
        rig.model.begin()
        await settle("both reads to wait") { forms.waiting == 1 && lists.waiting == 1 }
        rig.model.selectRuntime(Self.database)
        await settle("both new reads to wait") { forms.waiting == 2 && lists.waiting == 2 }

        forms.release(FixtureCreateForm.database)
        lists.release(FixtureCreateForm.secrets)
        await Task.yield()
        #expect(rig.model.phase == .loadingForm)
        #expect(rig.model.copyFromOptions.isEmpty)

        forms.release(FixtureCreateForm.database)
        lists.release([])
        await settle("editing") { rig.model.phase == .editing }
    }

    // MARK: - Create

    @MainActor
    private func creating(
        _ rig: CreateRig, secrets: [SecretFieldInput] = []
    ) async {
        await rig.model.create(secrets: secrets)
    }

    @Test func createSendsTheDraftAndFollowsTheOperationToItsEnd() async throws {
        let rig = await editing()
        await creating(
            rig, secrets: [SecretFieldInput(field: "superuserPassword", input: .generate)])
        let first = try #require(rig.sent.first)
        guard case .instanceCreate(let params) = first.call else {
            Issue.record("the first mutation is not instance.create")
            return
        }
        #expect(params.name == "postgresql-17")
        #expect(params.provider == ProviderSelector(name: "official/postgresql", revision: nil))
        #expect(params.runtimeId == Self.mysql)
        #expect(params.secrets == [SecretFieldInput(field: "superuserPassword", input: .generate)])
        #expect(rig.model.phase == .creating(CreateRig.operationID))
        #expect(rig.model.isReadOnly)

        rig.emit(CreateRig.operation(state: .running, stage: "pin", revision: 2))
        await settle("the stage") { rig.model.operation?.stage == "pin" }
        #expect(rig.model.progress?.runningIndex == 1)
        #expect(rig.model.secretsAreStored == false)
        rig.emit(CreateRig.operation(state: .running, stage: "layout", revision: 3))
        await settle("the stage") { rig.model.secretsAreStored }

        let instance = FixtureID.uuid(kind: FixtureID.instance, number: 20)
        rig.emit(
            CreateRig.operation(state: .succeeded, stage: nil, instance: instance, revision: 4))
        await settle("the outcome") { !rig.finished.isEmpty }
        #expect(rig.model.phase == .finished)
        #expect(
            rig.finished == [
                CreateOutcome(name: "postgresql-17", instanceID: instance, start: .accepted)
            ])
        let start = try #require(rig.sent.last)
        #expect(
            start.call
                == .instanceStart(StartInstanceParams(instanceId: instance, expectedRevision: nil)))
        #expect(start.key == CreateInstanceModel.startKey(createKey: first.key))
        #expect(start.key.count == 64)
    }

    @Test func createWithoutTheStartToggleSendsNoStart() async {
        let rig = await editing()
        rig.model.setStartAfterCreating(false)
        await settle("a preflight") {
            rig.model.currentPreflight != nil && rig.service.preflightCalls.count == 2
        }
        await creating(rig)
        let instance = FixtureID.uuid(kind: FixtureID.instance, number: 20)
        rig.emit(CreateRig.operation(state: .succeeded, stage: nil, instance: instance))
        await settle("the outcome") { !rig.finished.isEmpty }
        #expect(rig.sent.count == 1)
        #expect(rig.finished.first?.start == .notRequested)
        #expect(rig.finished.first?.feedback.message == "“postgresql-17” created.")
    }

    @Test func aStartThatFailsAfterAGoodCreateStillReportsTheCreate() async {
        let rig = await editing()
        rig.answer = { call, _ in
            switch call {
            case .instanceCreate:
                .completed(
                    .instanceCreate(
                        InstanceActionAccepted(
                            accepted: OperationAccepted(
                                operation: CreateRig.operation(state: .running, stage: "validate"),
                                replayed: false), alreadyInState: false)))
            default: .rejected(.create("supervisor.endpoint_in_use", "port 5432 is in use"))
            }
        }
        await creating(rig)
        let instance = FixtureID.uuid(kind: FixtureID.instance, number: 20)
        rig.emit(CreateRig.operation(state: .succeeded, stage: nil, instance: instance))
        await settle("the outcome") { !rig.finished.isEmpty }
        let outcome = rig.finished[0]
        #expect(outcome.start == .failed(detail: "Port 5432 is in use."))
        #expect(outcome.feedback.tone == .caution)
        #expect(outcome.feedback.message == "“postgresql-17” created, but it didn’t start.")
    }

    @Test func theFeedbackNamesTheStartWhenItWasAccepted() {
        let outcome = CreateOutcome(name: "orders", instanceID: nil, start: .accepted)
        #expect(outcome.feedback.message == "“orders” created. Starting it now.")
    }

    @Test func aRefusalReturnsTheFormWithABannerAndAsksAgain() async {
        let rig = await editing()
        rig.answer = { _, _ in
            .rejected(.create("supervisor.endpoint_conflict", "port 5432 is used by pg17-dev"))
        }
        let before = rig.service.preflightCalls.count
        await creating(rig)
        #expect(rig.model.phase == .editing)
        #expect(rig.model.banner?.tone == .critical)
        #expect(rig.model.banner?.code == "supervisor.endpoint_conflict")
        await settle("another preflight") { rig.service.preflightCalls.count == before + 1 }
    }

    @Test func aRefusedCreateLeavesAnEnterSecretOffUntilTheTextIsTypedAgain() async throws {
        let rig = await editing()
        let schema = try #require(rig.model.schema)
        schema.setSecret(
            "superuserPassword", SecretChoice(source: .enter, reference: nil, isValid: true))
        await settle("an Enter preflight") {
            rig.service.preflightCalls.last?.secrets?.contains(
                SecretPlan(field: "superuserPassword", source: .value)) == true
                && rig.model.createDisabledReason == nil
        }
        rig.answer = { _, _ in .rejected(.create("instance.name_taken", "the name is taken")) }
        let before = rig.service.preflightCalls.count
        await creating(
            rig,
            secrets: [SecretFieldInput(field: "superuserPassword", input: .value("typed-text"))])
        await settle("another preflight") { rig.service.preflightCalls.count == before + 1 }

        #expect(rig.model.phase == .editing)
        #expect(
            schema.secretChoice("superuserPassword")
                == SecretChoice(source: .enter, reference: nil, isValid: false))
        #expect(rig.model.createDisabledReason != nil)
        #expect(
            rig.service.preflightCalls.last?.secrets?.contains(
                SecretPlan(field: "superuserPassword", source: .value)) == true)

        schema.setSecret(
            "superuserPassword", SecretChoice(source: .enter, reference: nil, isValid: true))
        #expect(rig.model.createDisabledReason == nil)
    }

    @Test func aRefusedCreateKeepsACopyFromSecretAsItWas() async throws {
        let rig = await editing()
        let schema = try #require(rig.model.schema)
        schema.setSecret(
            "superuserPassword",
            SecretChoice(source: .copyFrom, reference: "local:user/x", isValid: true))
        rig.answer = { _, _ in .rejected(.create("resource.busy")) }
        await creating(
            rig,
            secrets: [
                SecretFieldInput(field: "superuserPassword", input: .copyFrom("local:user/x"))
            ])

        #expect(
            schema.secretChoice("superuserPassword")
                == SecretChoice(source: .copyFrom, reference: "local:user/x", isValid: true))
        await settle("a current preflight") { rig.model.createDisabledReason == nil }
    }

    @Test func aNewRuntimeStartsEverySecretOnGenerateEvenAfterAnEnterChoice() async throws {
        let rig = await editing()
        let first = try #require(rig.model.schema)
        first.setSecret(
            "superuserPassword", SecretChoice(source: .enter, reference: nil, isValid: true))
        rig.model.selectRuntime(Self.database)
        await settle("the new form") { rig.model.schema.map { $0 !== first } == true }

        let second = try #require(rig.model.schema)
        #expect(second.secretChoice("superuserPassword").source == .generate)
        #expect(second.secretPlans() == [SecretPlan(field: "superuserPassword", source: .generate)])
    }

    @Test func aCreateThatWasNotSentKeepsTheFormWithACautionBanner() async {
        let rig = await editing()
        rig.answer = { _, _ in .notSent(ClientErrorCatalog.notConnected()) }
        await creating(rig)
        #expect(rig.model.phase == .editing)
        #expect(rig.model.banner?.tone == .caution)
        #expect(rig.model.banner?.title == MutationFeedback.notSent)
    }

    @Test func aRetryAfterAnUnknownOutcomeUsesTheSameKeyUntilTheDraftChanges() async {
        let rig = await editing()
        rig.answer = { _, key in .outcomeUnknown(ClientErrorCatalog.notConnected(), key: key) }
        await creating(rig)
        #expect(rig.model.banner?.title == MutationFeedback.outcomeUnknown)
        await settle("a current preflight") { rig.model.currentPreflight != nil }
        await creating(rig)
        #expect(rig.sent.count == 2)
        #expect(rig.sent[0].key == rig.sent[1].key)

        rig.model.setName("another-name")
        await settle("a current preflight") { rig.model.currentPreflight != nil }
        await creating(rig)
        #expect(rig.sent[2].key != rig.sent[0].key)
    }

    @Test func aFailedOperationReturnsTheFormAndPointsAtTheStageThatFailed() async {
        let rig = await editing()
        await creating(rig)
        rig.emit(
            CreateRig.operation(
                state: .failed, stage: "secrets",
                failure: OperationFailureInfo(
                    reason: "step_failed", retryable: true,
                    error: .create("secret.store_locked", "the keychain is locked")),
                revision: 2))
        await settle("the form") { rig.model.phase == .editing }
        #expect(rig.model.banner?.code == "secret.store_locked")
        #expect(rig.model.banner?.message == "The keychain is locked.")
        #expect(rig.model.failedStageKinds == [.secret])
        rig.model.setName("another-name")
        #expect(rig.model.failedStageKinds.isEmpty)
    }

    @Test func aCancelledOperationSaysNothingWasKept() async {
        let rig = await editing()
        await creating(rig)
        rig.emit(CreateRig.operation(state: .cancelled, stage: "init", revision: 2))
        await settle("the form") { rig.model.phase == .editing }
        #expect(rig.model.banner?.title == "Creation was cancelled. Nothing was kept.")
        #expect(rig.model.banner?.tone == .neutral)
    }

    @Test func theClickHandsTheSecretsOverAndTheViewClearsThem() async {
        let rig = await editing()
        var cleared = false
        let action = rig.model.createAction(
            secrets: { [SecretFieldInput(field: "superuserPassword", input: .generate)] },
            didSend: { cleared = true })
        action.invoke(gate: .available, present: { _ in })
        await settle("the send") { !rig.sent.isEmpty }
        #expect(cleared)
        #expect("\(rig.model)".contains("hunter2") == false)
    }

    // MARK: - Cancel

    @Test func cancelDuringTheOperationAsksBeforeItStops() async {
        let rig = await editing()
        await creating(rig)
        guard let action = rig.model.cancelAction(gate: .available, close: {}),
            let confirmation = action.confirmation
        else {
            Issue.record("the cancel action has no confirmation")
            return
        }
        #expect(action.title == "Cancel…")
        #expect(action.mutates)
        #expect(confirmation.title == "Stop creating this instance?")
        #expect(
            confirmation.message
                == "Everything created so far is removed. Initialization in progress finishes first."
        )
        #expect(confirmation.actionTitle == "Stop")
        action.perform()
        await settle("the cancel") { rig.sent.count == 2 }
        #expect(
            rig.sent[1].call
                == .operationCancel(CancelOperationParams(operationId: CreateRig.operationID)))
    }

    @Test func whileTheDaemonCannotBeReachedCancelBecomesClose() async {
        let rig = await editing()
        await creating(rig)
        var closed = false
        let blocked = MutationAvailability.blocked(
            reason: "stacklyd is unavailable.", since: Date())
        guard let action = rig.model.cancelAction(gate: blocked, close: { closed = true }) else {
            Issue.record("no close action")
            return
        }
        #expect(action.title == "Close")
        #expect(action.mutates == false)
        #expect(action.confirmation == nil)
        action.perform()
        #expect(closed)
        #expect(rig.sent.count == 1)
    }

    @Test func outsideTheOperationTheSheetKeepsItsPlainCancel() async {
        let rig = await editing()
        #expect(rig.model.cancelAction(gate: .available, close: {}) == nil)
    }

    @Test func theCreateActionReadsTheGateAndTheBusyPhase() async {
        let rig = await editing()
        let action = rig.model.createAction(secrets: { [] })
        #expect(action.id == "create")
        #expect(action.title == "Create")
        #expect(action.mutates)
        #expect(action.availability(gate: .available) == .enabled)
        await creating(rig)
        let busy = rig.model.createAction(secrets: { [] })
        #expect(busy.availability(gate: .available) == .busy)
    }

    @Test func theStartKeyIsTheHexSha256OfTheCreateKeyAndStart() {
        // sha256("abc:start"), lowercase hex.
        #expect(
            CreateInstanceModel.startKey(createKey: "abc")
                == "4a3a2491fb167b157187071d489e147d45fcc682bed2471532adf3dada375f3f")
    }
}
