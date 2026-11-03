import AppKit
import Foundation
import Testing

@testable import StacklyUI

/// The activation policy follows the open windows (spec 0032 AC-11, AC-12): `accessory` when
/// the last window closes, `regular` and active before a window opens, and a login item
/// launch is told by the open application event alone.
@MainActor
struct ActivationPolicyControllerTests {
    private final class Recorder {
        var policies: [ActivationPolicyController.Policy] = []
        var activations = 0
    }

    private func controller(_ recorder: Recorder) -> ActivationPolicyController {
        ActivationPolicyController(
            host: .init(
                setPolicy: { recorder.policies.append($0) },
                activate: { recorder.activations += 1 }))
    }

    @Test func theLastWindowClosingMakesTheAppAccessory() {
        let recorder = Recorder()
        let controller = controller(recorder)
        let main = NSObject()
        let settings = NSObject()

        controller.windowOpened(ObjectIdentifier(main))
        controller.windowOpened(ObjectIdentifier(settings))
        controller.windowClosed(ObjectIdentifier(main))
        #expect(controller.policy == .regular)
        controller.windowClosed(ObjectIdentifier(settings))

        #expect(controller.policy == .accessory)
        #expect(controller.openWindowCount == 0)
        #expect(recorder.policies == [.accessory])
    }

    @Test func aWindowThatStaysOpenKeepsTheDockIconEvenWhenMinimized() {
        // A minimized window or one in another Space is still open: no willClose came.
        let recorder = Recorder()
        let controller = controller(recorder)
        let main = NSObject()
        let gallery = NSObject()
        controller.windowOpened(ObjectIdentifier(main))
        controller.windowOpened(ObjectIdentifier(gallery))

        controller.windowClosed(ObjectIdentifier(gallery))

        #expect(controller.policy == .regular)
        #expect(recorder.policies.isEmpty)
    }

    @Test func preparingToShowAWindowMakesTheAppRegularAndActive() {
        let recorder = Recorder()
        let controller = controller(recorder)
        let only = NSObject()
        controller.windowOpened(ObjectIdentifier(only))
        controller.windowClosed(ObjectIdentifier(only))
        #expect(controller.policy == .accessory)

        controller.prepareToShowWindow()

        #expect(controller.policy == .regular)
        #expect(recorder.policies == [.accessory, .regular])
        #expect(recorder.activations == 1)
    }

    @Test func preparingWhileRegularOnlyActivates() {
        let recorder = Recorder()
        let controller = controller(recorder)

        controller.prepareToShowWindow()
        controller.prepareToShowWindow()

        #expect(recorder.policies.isEmpty)
        #expect(recorder.activations == 2)
    }

    @Test func aLoginItemLaunchIsRecognizedByItsOpenApplicationEvent() {
        func openApplication(_ data: AEKeyword?) -> NSAppleEventDescriptor {
            let event = NSAppleEventDescriptor(
                eventClass: AEEventClass(kCoreEventClass), eventID: AEEventID(kAEOpenApplication),
                targetDescriptor: nil, returnID: AEReturnID(kAutoGenerateReturnID),
                transactionID: AETransactionID(kAnyTransactionID))
            if let data {
                event.setParam(
                    NSAppleEventDescriptor(enumCode: OSType(data)),
                    forKeyword: AEKeyword(keyAEPropData))
            }
            return event
        }

        #expect(LaunchOrigin.isLoginItem(openApplication(AEKeyword(keyAELaunchedAsLogInItem))))
        // A plain launch, a service item launch, another event, and no event all open the window.
        #expect(LaunchOrigin.isLoginItem(openApplication(nil)) == false)
        #expect(
            LaunchOrigin.isLoginItem(openApplication(AEKeyword(keyAELaunchedAsServiceItem)))
                == false)
        let other = NSAppleEventDescriptor(
            eventClass: AEEventClass(kCoreEventClass), eventID: AEEventID(kAEReopenApplication),
            targetDescriptor: nil, returnID: AEReturnID(kAutoGenerateReturnID),
            transactionID: AETransactionID(kAnyTransactionID))
        #expect(LaunchOrigin.isLoginItem(other) == false)
        #expect(LaunchOrigin.isLoginItem(nil) == false)
    }

    @Test func theQueueHandsADestinationToOneWindowOnce() {
        let queue = ShellDestinationQueue()
        let id = UUID()

        queue.request(.instance(id))
        #expect(queue.pending == .instance(id))
        #expect(queue.take() == .instance(id))
        #expect(queue.pending == nil)
        #expect(queue.take() == nil)
    }
}
