import Foundation
import StacklyClient
import Testing

@testable import StacklyUI

/// The ways in to a duplicate (spec 0024 AC-13): the More item of the detail page, the
/// Duplicate… command of the Instance menu, and the presenter that opens the sheet.
@MainActor
struct DuplicateEntryPointTests {
    private static let source = DuplicateSourceRef(
        id: FixtureID.uuid(kind: FixtureID.instance, number: 3), name: "pg17-dev")

    private func presenter() async -> (NewInstancePresenter, CreateRig) {
        let rig = CreateRig()
        let duplicate = try? contractDuplicate("fake")
        rig.service.onDuplicate = { _ async throws(ClientError) -> DuplicateForm in
            guard let duplicate else { throw .rejected(ClientErrorCatalog.notConnected()) }
            return duplicate
        }
        await rig.load()
        return (NewInstancePresenter(model: rig.model), rig)
    }

    private func publish(duplicate: StacklyAction?) -> [CommandID: StacklyAction] {
        InstancesCommandActions.publish(
            showsInspector: true, toggleInspector: {}, sort: .default, setSort: { _ in },
            focusSearch: {}, target: nil, duplicate: duplicate
        ).actions
    }

    // MARK: - The action

    @Test func theActionIsAMutationWithoutConfirmationTitledLikeTheMenuItem() async {
        let (presenter, _) = await presenter()
        let action = presenter.duplicateAction.stacklyAction(for: Self.source)
        #expect(action.id == "duplicateInstance")
        #expect(action.title == "Duplicate…")
        #expect(action.title == CommandMap.entry(for: .duplicateInstance).title)
        #expect(action.mutates)
        #expect(action.confirmation == nil)
        #expect(action.disabledReason == nil)
        #expect(action.availability(gate: .available) == .enabled)
    }

    @Test func theMutationGateKeepsItOffWithoutAReasonOfItsOwn() async {
        let (presenter, _) = await presenter()
        let action = presenter.duplicateAction.stacklyAction(for: Self.source)
        let gate = MutationAvailability.blocked(reason: "No connection.", since: .distantPast)
        #expect(
            action.availability(gate: gate)
                == .disabled(reason: "No connection.", showsReason: false))
    }

    // MARK: - The More menu

    @Test func theMoreMenuListsDuplicateRightAfterCopyConnectionString() async {
        let (presenter, _) = await presenter()
        let copy = StacklyAction(
            id: "copyConnectionString", title: "Copy Connection String", mutates: false,
            perform: {})
        let items = InstanceDetailPage.moreItems(
            copyConnection: copy, duplicate: presenter.duplicateAction, source: Self.source)
        #expect(items.map(\.title) == ["Copy Connection String", "Duplicate…"])
    }

    @Test func theMoreMenuHasNoDuplicateWhereTheWindowHasNoSheet() {
        let copy = StacklyAction(
            id: "copyConnectionString", title: "Copy Connection String", mutates: false,
            perform: {})
        let items = InstanceDetailPage.moreItems(
            copyConnection: copy, duplicate: nil, source: Self.source)
        #expect(items.map(\.title) == ["Copy Connection String"])
    }

    // MARK: - The command

    @Test func theInstancesPagePublishesDuplicateOnlyWithATarget() async {
        let (presenter, _) = await presenter()
        #expect(publish(duplicate: nil)[.duplicateInstance] == nil)
        let published = publish(
            duplicate: presenter.duplicateAction.stacklyAction(for: Self.source))
        #expect(published[.duplicateInstance]?.id == "duplicateInstance")
        #expect(published[.duplicateInstance]?.mutates == true)
    }

    @Test func theCommandIsBuiltInGroupFourOfTheInstanceMenuWithCommandD() {
        let entry = CommandMap.entry(for: .duplicateInstance)
        #expect(entry.status == .built)
        #expect(entry.menu == .instance)
        #expect(entry.group == 4)
        #expect(entry.shortcut == CommandShortcut("d", .command))
        #expect(entry.scope == .page([.instances]))
        #expect(CommandMap.helpText(for: .duplicateInstance) == "Duplicate… (⌘D)")
    }

    // MARK: - The presenter

    @Test func presentingADuplicateOpensTheSheetInDuplicateMode() async {
        let (presenter, rig) = await presenter()
        presenter.duplicateAction(Self.source)
        #expect(presenter.isPresented)
        #expect(rig.model.mode == .duplicate(Self.source))
        await settle("the form") { rig.model.phase == .editing }
        #expect(rig.service.duplicateCalls == [Self.source.id])
        #expect(rig.service.formCalls.isEmpty)
    }

    @Test func closingTheDuplicateSheetEndsTheModelAndTheNextCreateStartsClean() async {
        let (presenter, rig) = await presenter()
        presenter.presentDuplicate(of: Self.source)
        await settle("the form") { rig.model.phase == .editing }
        presenter.isPresented = false
        #expect(rig.model.phase == .choosingRuntime)
        #expect(rig.model.mode == .create(software: nil))

        presenter.present()
        await settle("the form") { rig.model.phase == .editing }
        #expect(rig.model.duplicateSource == nil)
        #expect(rig.model.schema?.copyFromOptionsByField.isEmpty == true)
    }

    @Test func aCreateThatRunsComesBackWhenDuplicateIsChosenAgain() async {
        let (presenter, rig) = await presenter()
        presenter.presentDuplicate(of: Self.source)
        await settle("the first preflight") { rig.model.currentPreflight != nil }
        await rig.model.create(secrets: [])
        presenter.isPresented = false
        #expect(rig.model.phase == .creating(CreateRig.operationID))

        presenter.presentDuplicate(of: Self.source)
        #expect(presenter.isPresented)
        #expect(rig.service.duplicateCalls.count == 1)
    }
}
