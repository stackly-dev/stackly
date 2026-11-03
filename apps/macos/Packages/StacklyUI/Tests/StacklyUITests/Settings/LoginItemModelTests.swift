import Foundation
import Testing

@testable import StacklyUI

/// Open Stackly at login (spec 0032 AC-12): the state is always read from the OS, a refusal
/// puts the toggle back, and a user's change in System Settings shows after a refresh.
@Suite(.timeLimit(.minutes(1)))
struct LoginItemModelTests {
    @Test func nothingIsKnownBeforeTheFirstRead() {
        let model = LoginItemModel(controller: FakeLoginItem())
        #expect(model.status == nil)
        #expect(model.isOn == false)
    }

    @Test func refreshReadsTheOSStateAndNeverChangesIt() async {
        let fake = FakeLoginItem(status: .enabled)
        let model = LoginItemModel(controller: fake)
        await model.refresh()
        #expect(model.status == .enabled)
        #expect(model.isOn)
        #expect(fake.calls == [.status])
    }

    @Test func turningOnRegistersAndShowsWhatTheOSSays() async {
        let fake = FakeLoginItem(status: .notRegistered, afterRegister: .enabled)
        let model = LoginItemModel(controller: fake)
        await model.refresh()
        await model.setEnabled(true)
        #expect(model.status == .enabled)
        #expect(model.isOn)
        #expect(model.error == nil)
        #expect(fake.calls == [.status, .register])
    }

    @Test func aRegistrationThatNeedsApprovalShowsOnWithLoginItems() async {
        let fake = FakeLoginItem(status: .notRegistered, afterRegister: .requiresApproval)
        let model = LoginItemModel(controller: fake)
        await model.setEnabled(true)
        #expect(model.isOn)
        #expect(model.needsApproval)
        model.openLoginItems()
        #expect(fake.calls.last == .openLoginItems)
    }

    @Test func turningOffUnregistersWithoutTouchingTheDaemonOrAnyInstance() async {
        let fake = FakeLoginItem(status: .enabled)
        let model = LoginItemModel(controller: fake)
        await model.refresh()
        await model.setEnabled(false)
        #expect(model.status == .notRegistered)
        #expect(model.isOn == false)
        #expect(fake.calls == [.status, .unregister])
    }

    @Test func aRefusalKeepsTheSentenceAndReadsTheStateAgain() async {
        let fake = FakeLoginItem(status: .notRegistered)
        fake.failNext(
            with: LoginItemError(
                message:
                    "macOS returned SMAppServiceErrorDomain 1 while trying to add Stackly to Login Items."
            ))
        let model = LoginItemModel(controller: fake)
        await model.setEnabled(true)
        #expect(model.isOn == false)
        #expect(model.error?.contains("SMAppServiceErrorDomain 1") == true)
        #expect(fake.calls == [.register, .status])
    }

    @Test func theNextChangeClearsTheSentenceOfTheLastOne() async {
        let fake = FakeLoginItem(status: .notRegistered)
        fake.failNext(with: LoginItemError(message: "no"))
        let model = LoginItemModel(controller: fake)
        await model.setEnabled(true)
        await model.setEnabled(true)
        #expect(model.error == nil)
        #expect(model.isOn)
    }

    @Test func aChangeTheUserMakesInSystemSettingsShowsAfterARefresh() async {
        let fake = FakeLoginItem(status: .enabled)
        let model = LoginItemModel(controller: fake)
        await model.refresh()
        fake.setStatus(.notRegistered)
        await model.refresh()
        #expect(model.isOn == false)
    }
}
