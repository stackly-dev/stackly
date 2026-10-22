import Foundation
import StacklyClient
import Testing

@testable import StacklyUI

/// The New Instance sheet in duplicate mode over a scripted service (spec 0024 AC-12): the
/// load task, the draft the form starts from, Copy from for the source references, and the
/// way a duplicate ends.
struct CreateInstanceDuplicateTests {
    private static let source = DuplicateSourceRef(
        id: UUID(uuidString: "0192f3a6-0000-7000-8000-0000000000b1")!, name: "fake-a")
    private static let sourceReference =
        "local:instance/0192f3a6-0000-7000-8000-0000000000b1/auth-token"

    private func list(_ references: [(String, SecretStatus)]) -> [SecretRefSummary] {
        references.map { reference, status in
            SecretRefSummary(
                reference: reference, ownerKind: .user, ownerId: nil, purpose: "user",
                status: status, statusObservedAt: FixtureClock.minutesAgo(5), valueOrigin: nil,
                valueUpdatedAt: nil, usedBy: nil, revision: 1)
        }
    }

    /// A rig whose service answers the `fake` duplicate fixture, with the sheet opened on it.
    @MainActor
    private func opened(
        form: String = "fake", prepare: (CreateRig, DuplicateForm) -> Void = { _, _ in }
    ) async throws -> (CreateRig, DuplicateForm) {
        let duplicate = try contractDuplicate(form)
        let rig = CreateRig()
        rig.service.onDuplicate = { _ in duplicate }
        prepare(rig, duplicate)
        await rig.load()
        rig.model.beginDuplicate(of: Self.source)
        return (rig, duplicate)
    }

    @MainActor
    private func editing(
        prepare: (CreateRig, DuplicateForm) -> Void = { _, _ in }
    ) async throws -> (CreateRig, DuplicateForm) {
        let (rig, duplicate) = try await opened(prepare: prepare)
        await settle("the first preflight") { rig.model.currentPreflight != nil }
        return (rig, duplicate)
    }

    private static func external(_ runtime: RuntimeSummary) -> RuntimeSummary {
        RuntimeSummary(
            id: runtime.id, revision: runtime.revision, software: runtime.software,
            displayName: runtime.displayName, version: runtime.version, line: runtime.line,
            target: runtime.target, installMode: .external, state: runtime.state,
            artifactSha256: runtime.artifactSha256, installedAt: runtime.installedAt,
            lastVerifiedAt: runtime.lastVerifiedAt, support: runtime.support,
            supportReason: runtime.supportReason, usedBy: runtime.usedBy,
            dependentRuntimes: runtime.dependentRuntimes)
    }

    // MARK: - Loading

    @Test func theSheetLoadsTheFormAndTheListTogetherAndSkipsTheRuntimePicker() async throws {
        let (rig, duplicate) = try await opened()
        #expect(rig.model.mode == .duplicate(Self.source))
        #expect(rig.model.phase == .loadingForm || rig.model.phase == .editing)
        await settle("the form") { rig.model.phase == .editing }
        await settle("the list") { rig.service.listCalls == 1 }
        #expect(rig.service.duplicateCalls == [Self.source.id])
        #expect(rig.service.formCalls.isEmpty)
        #expect(rig.model.runtimeID == duplicate.form.runtime.id)
    }

    @Test func theFormShowsNothingUntilTheDaemonAnswers() async throws {
        let gate = await Gate<DuplicateForm>()
        let duplicate = try contractDuplicate("fake")
        let rig = CreateRig()
        rig.service.onDuplicate = { _ in await gate.wait() }
        await rig.load()
        rig.model.beginDuplicate(of: Self.source)
        #expect(rig.model.phase == .loadingForm)
        #expect(rig.model.isCreateBusy)
        #expect(rig.model.schema == nil)
        await settle("the call") { gate.waiting == 1 }
        gate.release(duplicate)
        await settle("the form") { rig.model.phase == .editing }
    }

    @Test func theFormStartsFromTheDraftNotFromTheDefaultsOfTheForm() async throws {
        let (rig, duplicate) = try await editing()
        let model = rig.model
        #expect(model.name == "fake-a-copy-2")
        #expect(model.name == duplicate.draft.name)
        let schema = try #require(model.schema)
        #expect(schema.text("greeting") == "hello from fake-a")
        #expect(schema.text("label") == "fake")
        #expect(schema.text("crashAfterMs") == "0")
        #expect(model.currentPreflight == FixtureCreateForm.ready)
    }

    @Test func everyPortStartsAutomaticAndFixedFillsTheDefaultPort() async throws {
        let (rig, _) = try await editing()
        #expect(rig.model.endpoints.map(\.name) == ["http"])
        #expect(rig.model.endpoints.allSatisfy { $0.mode == .automatic })
        #expect(rig.model.endpoints.first?.portText == "18080")
        rig.model.setMode(.fixed, of: "http")
        #expect(rig.model.endpoints.first?.request?.port == .fixed(18080))
    }

    @Test func thePreflightDraftPinsTheProviderRevisionOfTheSource() async throws {
        let (rig, duplicate) = try await editing()
        let draft = try #require(rig.service.preflightCalls.first)
        #expect(draft.provider == duplicate.draft.provider)
        #expect(draft.provider.revision == "0.3.0")
        #expect(draft.runtimeId == duplicate.draft.runtimeId)
        #expect(draft.displayName == nil)
        #expect(draft.endpoints == [EndpointRequest(name: "http", port: .auto)])
        #expect(draft.secrets == [SecretPlan(field: "authToken", source: .generate)])
        #expect(draft.settings.members["greeting"] == .string("hello from fake-a"))
        #expect(draft.settings.members["authToken"] == nil)
    }

    @Test func theRuntimeRowNamesTheSourceAndTheSubtitleSaysNoDataIsCopied() async throws {
        let (rig, duplicate) = try await editing()
        let runtime = duplicate.form.runtime
        #expect(
            rig.model.duplicateRuntimeLabel
                == "\(duplicate.form.provider.displayName) \(runtime.version) · same as “fake-a”")
        #expect(
            rig.model.duplicateSubtitle
                == "Settings copied from “fake-a”. Its own data directory, ports and secrets; no data is copied."
        )
        #expect(rig.model.runtime == runtime)
        #expect(rig.model.tile != nil)
    }

    @Test func anExternalRuntimeIsMarkedInTheRuntimeRow() async throws {
        let duplicate = try contractDuplicate("fake")
        let external = CreateForm(
            provider: duplicate.form.provider,
            runtime: Self.external(duplicate.form.runtime),
            suggestedName: duplicate.form.suggestedName, dataPath: duplicate.form.dataPath,
            endpoints: duplicate.form.endpoints, fields: duplicate.form.fields,
            capabilities: duplicate.form.capabilities)
        let rig = CreateRig()
        rig.service.onDuplicate = { _ in
            DuplicateForm(
                source: duplicate.source, form: external, draft: duplicate.draft,
                secrets: duplicate.secrets)
        }
        await rig.load()
        rig.model.beginDuplicate(of: Self.source)
        await settle("the form") { rig.model.phase == .editing }
        #expect(rig.model.duplicateRuntimeLabel?.contains("(external) · same as “fake-a”") == true)
    }

    @Test func aPostgresqlDuplicateRendersAFormWithItsSettingsFromTheSource() async throws {
        let duplicate = try contractDuplicate("postgresql")
        let rig = CreateRig()
        rig.service.onDuplicate = { _ in duplicate }
        await rig.load()
        rig.model.beginDuplicate(
            of: DuplicateSourceRef(id: duplicate.source.instanceId, name: "pg17-dev"))
        await settle("the form") { rig.model.phase == .editing }
        let schema = try #require(rig.model.schema)
        #expect(schema.text("maxConnections") == "200")
        #expect(schema.text("logStatement") == "ddl")
        #expect(rig.model.name == "pg17-dev-copy")
        #expect(rig.model.endpoints.allSatisfy { $0.mode == .automatic })
    }

    // MARK: - Secrets

    @Test func everySecretStartsOnGenerateWithCopyFromPointingAtTheSource() async throws {
        let (rig, _) = try await editing()
        let choice = try #require(rig.model.schema?.secretChoice("authToken"))
        #expect(choice.source == .generate)
        #expect(choice.reference == Self.sourceReference)
        #expect(
            rig.model.schema?.secretPlans() == [SecretPlan(field: "authToken", source: .generate)])
    }

    @Test func copyFromOffersOnlyTheSourceWhileTheListIsOutAndWhenItFails() async throws {
        let gate = await Gate<[SecretRefSummary]>()
        let (rig, _) = try await editing { rig, _ in
            rig.service.onSecrets = { await gate.wait() }
        }
        #expect(rig.model.copyFromOptions(for: "authToken") == [Self.sourceReference])
        #expect(rig.model.schema?.copyFromOptions(for: "authToken") == [Self.sourceReference])
        #expect(
            rig.model.copyFromLabels == [Self.sourceReference: "Auth token of “fake-a”"])

        let fake = try contractDuplicate("fake")
        let failing = CreateRig()
        failing.service.onDuplicate = { _ in fake }
        failing.service.onSecrets = { () async throws(ClientError) -> [SecretRefSummary] in
            throw .rejected(.create("secret.unavailable"))
        }
        await failing.load()
        failing.model.beginDuplicate(of: Self.source)
        await settle("the form") { failing.model.phase == .editing }
        await settle("the list") { failing.service.listCalls == 1 }
        #expect(failing.model.phase == .editing)
        #expect(failing.model.copyFromOptions(for: "authToken") == [Self.sourceReference])
    }

    @Test func theListIsAddedBehindTheSourceWithoutDuplicatesOrChangingTheChoice() async throws {
        let (rig, _) = try await editing { rig, _ in
            rig.service.onSecrets = { [list] in
                list([
                    ("local:user/shared-token", .present),
                    (Self.sourceReference, .present),
                    ("local:user/gone", .missing),
                ])
            }
        }
        await settle("the list") { !rig.model.copyFromOptions.isEmpty }
        #expect(
            rig.model.copyFromOptions(for: "authToken") == [
                Self.sourceReference, "local:user/shared-token",
            ])
        #expect(
            rig.model.schema?.copyFromOptions(for: "authToken")
                == rig.model.copyFromOptions(for: "authToken"))
        let choice = try #require(rig.model.schema?.secretChoice("authToken"))
        #expect(choice.source == .generate)
        #expect(choice.reference == Self.sourceReference)
    }

    @Test func aSourceReferenceThatIsMissingInTheListStaysFirst() async throws {
        let (rig, _) = try await editing { rig, _ in
            rig.service.onSecrets = { [list] in
                list([(Self.sourceReference, .missing), ("local:user/shared-token", .present)])
            }
        }
        await settle("the list") { rig.model.copyFromOptions == ["local:user/shared-token"] }
        #expect(
            rig.model.copyFromOptions(for: "authToken") == [
                Self.sourceReference, "local:user/shared-token",
            ])
    }

    @Test func theLabelOfTheSourceBeatsTheLabelOfTheList() async throws {
        let (rig, _) = try await editing { rig, _ in
            rig.service.onSecrets = {
                [
                    SecretRefSummary(
                        reference: Self.sourceReference, ownerKind: .instance,
                        ownerId: Self.source.id, purpose: "auth-token", status: .present,
                        statusObservedAt: FixtureClock.minutesAgo(5), valueOrigin: nil,
                        valueUpdatedAt: nil, usedBy: nil, revision: 1)
                ]
            }
        }
        await settle("the list") { !rig.model.copyFromOptions.isEmpty }
        #expect(rig.model.copyFromLabels[Self.sourceReference] == "Auth token of “fake-a”")
    }

    @Test func aFieldWithoutASourceReferenceKeepsTheListOfACreate() async throws {
        let duplicate = try contractDuplicate("fake")
        let noReference = DuplicateForm(
            source: duplicate.source, form: duplicate.form, draft: duplicate.draft,
            secrets: [DuplicateSecret(field: "authToken", sourceReference: nil)])
        let rig = CreateRig()
        rig.service.onDuplicate = { _ in noReference }
        rig.service.onSecrets = { [list] in list([("local:user/shared-token", .present)]) }
        await rig.load()
        rig.model.beginDuplicate(of: Self.source)
        await settle("the list") { !rig.model.copyFromOptions.isEmpty }
        #expect(rig.model.copyFromOptions(for: "authToken") == ["local:user/shared-token"])
        #expect(rig.model.schema?.secretChoice("authToken").reference == nil)

        let none = CreateRig()
        none.service.onDuplicate = { _ in noReference }
        none.service.onSecrets = { [] }
        await none.load()
        none.model.beginDuplicate(of: Self.source)
        await settle("the form") { none.model.phase == .editing }
        await settle("the list") { none.service.listCalls == 1 }
        #expect(none.model.copyFromOptions(for: "authToken").isEmpty)
    }

    // MARK: - Failing to load

    @Test func aFormThatFailsToLoadShowsTheDaemonMessageAndRetryReadsBothAgain() async throws {
        let duplicate = try contractDuplicate("fake")
        let rig = CreateRig()
        var attempts = 0
        rig.service.onDuplicate = { _ async throws(ClientError) -> DuplicateForm in
            attempts += 1
            if attempts == 1 {
                throw .rejected(
                    .create(
                        "instance.capability_unsupported",
                        "official/example runs one instance at a time"))
            }
            return duplicate
        }
        await rig.load()
        rig.model.beginDuplicate(of: Self.source)
        await settle("the failure") {
            if case .formFailed = rig.model.phase { true } else { false }
        }
        guard case .formFailed(let content) = rig.model.phase else { return }
        #expect(content.message == "Official/example runs one instance at a time.")
        #expect(content.code == "instance.capability_unsupported")
        #expect(rig.model.createDisabledReason == "Couldn’t load the form.")
        #expect(rig.model.mode == .duplicate(Self.source))

        rig.model.retryLoad()
        #expect(rig.model.phase == .loadingForm)
        await settle("the form") { rig.model.phase == .editing }
        #expect(rig.service.duplicateCalls.count == 2)
        #expect(rig.service.listCalls == 2)
        #expect(rig.model.name == "fake-a-copy-2")
    }

    @Test func aListThatFailsNeverFailsTheForm() async throws {
        let (rig, _) = try await editing { rig, _ in
            rig.service.onSecrets = { () async throws(ClientError) -> [SecretRefSummary] in
                throw .rejected(.create("secret.unavailable"))
            }
        }
        #expect(rig.model.phase == .editing)
    }

    @Test func aReplyForAnOlderLoadIsDropped() async throws {
        let gate = await Gate<DuplicateForm>()
        let duplicate = try contractDuplicate("fake")
        let rig = CreateRig()
        rig.service.onDuplicate = { _ in await gate.wait() }
        await rig.load()
        rig.model.beginDuplicate(of: Self.source)
        await settle("the call") { gate.waiting == 1 }
        rig.model.end()
        gate.release(duplicate)
        try await Task.sleep(for: .milliseconds(30))
        #expect(rig.model.phase == .choosingRuntime)
        #expect(rig.model.form == nil)
    }

    // MARK: - Footer

    @Test func theFooterReasonsOfACreateAboutRuntimesNeverShowInADuplicate() async throws {
        let (rig, _) = try await opened()
        #expect(rig.model.createDisabledReason == nil)  // loading is busy
        await settle("the first preflight") { rig.model.currentPreflight != nil }
        #expect(rig.model.createDisabledReason == nil)
        #expect(rig.model.hasNoInstalledRuntimes == false)
    }

    // MARK: - Finishing

    @Test func aDuplicateCreatesWithThePinnedProviderAndReportsTheSource() async throws {
        let (rig, duplicate) = try await editing()
        rig.model.setStartAfterCreating(false)
        await settle("a preflight") {
            rig.model.currentPreflight != nil && rig.service.preflightCalls.count == 2
        }
        await rig.model.create(secrets: [SecretFieldInput(field: "authToken", input: .generate)])
        let first = try #require(rig.sent.first)
        guard case .instanceCreate(let params) = first.call else {
            Issue.record("the first mutation is not instance.create")
            return
        }
        #expect(params.provider == duplicate.draft.provider)
        #expect(params.runtimeId == duplicate.draft.runtimeId)
        #expect(params.name == "fake-a-copy-2")
        #expect(params.endpoints == [EndpointRequest(name: "http", port: .auto)])
        #expect(params.secrets == [SecretFieldInput(field: "authToken", input: .generate)])

        let instance = FixtureID.uuid(kind: FixtureID.instance, number: 20)
        rig.emit(CreateRig.operation(state: .succeeded, stage: nil, instance: instance))
        await settle("the outcome") { !rig.finished.isEmpty }
        #expect(
            rig.finished == [
                CreateOutcome(
                    name: "fake-a-copy-2", instanceID: instance, start: .notRequested,
                    source: "fake-a")
            ])
        #expect(rig.finished[0].feedback.message == "“fake-a-copy-2” created from “fake-a”.")
        #expect(rig.sent.count == 1)
    }

    @Test func theStartToggleSendsTheStartAndTheFeedbackSaysSo() async throws {
        let (rig, _) = try await editing()
        await rig.model.create(secrets: [])
        let instance = FixtureID.uuid(kind: FixtureID.instance, number: 20)
        rig.emit(CreateRig.operation(state: .succeeded, stage: nil, instance: instance))
        await settle("the outcome") { !rig.finished.isEmpty }
        #expect(rig.finished[0].start == .accepted)
        #expect(
            rig.finished[0].feedback.message
                == "“fake-a-copy-2” created from “fake-a”. Starting it now.")
    }

    @Test func aStartThatFailsAfterADuplicateStillReportsTheCreate() async throws {
        let (rig, _) = try await editing()
        rig.answer = { call, _ in
            switch call {
            case .instanceCreate:
                .completed(
                    .instanceCreate(
                        InstanceActionAccepted(
                            accepted: OperationAccepted(
                                operation: CreateRig.operation(state: .running, stage: "validate"),
                                replayed: false), alreadyInState: false)))
            default: .rejected(.create("supervisor.endpoint_in_use", "port 18080 is in use"))
            }
        }
        await rig.model.create(secrets: [])
        let instance = FixtureID.uuid(kind: FixtureID.instance, number: 20)
        rig.emit(CreateRig.operation(state: .succeeded, stage: nil, instance: instance))
        await settle("the outcome") { !rig.finished.isEmpty }
        #expect(rig.finished[0].feedback.message == "“fake-a-copy-2” created, but it didn’t start.")
    }

    @Test func endingTheSheetReturnsTheModelToACreate() async throws {
        let (rig, _) = try await editing()
        rig.model.end()
        #expect(rig.model.mode == .create(software: nil))
        #expect(rig.model.duplicateSource == nil)
        #expect(rig.model.duplicateRuntimeLabel == nil)
        rig.model.begin(.create(software: "postgresql"))
        #expect(rig.model.mode == .create(software: "postgresql"))
        rig.model.begin(.duplicate(Self.source))
        #expect(rig.model.mode == .duplicate(Self.source))
    }

    @Test func aPlainCreateKeepsItsListAndItsLabels() async throws {
        let rig = CreateRig()
        await rig.load()
        rig.model.begin()
        await settle("the list") { !rig.model.copyFromOptions.isEmpty }
        #expect(rig.model.copyFromOptions(for: "superuserPassword") == rig.model.copyFromOptions)
        #expect(rig.model.duplicateSubtitle == nil)
    }
}
