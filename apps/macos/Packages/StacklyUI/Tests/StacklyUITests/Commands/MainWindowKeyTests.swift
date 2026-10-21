import AppKit
import Testing

@testable import StacklyUI

/// `MainWindowKey` follows the notifications of one window only (spec 0069, AC-4).
struct MainWindowKeyTests {
    private final class Keyed {
        var value = false
    }

    private func window() -> NSWindow {
        NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 100, height: 100), styleMask: [.titled],
            backing: .buffered, defer: true)
    }

    private func post(_ name: Notification.Name, _ window: NSWindow) {
        NotificationCenter.default.post(name: name, object: window)
    }

    @Test func itIsNotKeyUntilAWindowIsTracked() {
        let key = MainWindowKey(isKeyWindow: { _ in true })
        #expect(key.isKey == false)
        #expect(key.isKeyNow == false)
    }

    @Test func itStartsWithTheStateOfTheTrackedWindow() {
        let key = MainWindowKey(isKeyWindow: { _ in true })
        key.track(window())
        #expect(key.isKey)
        #expect(key.isKeyNow)
    }

    @Test func theWindowBecomingAndResigningKeyChangesIsKey() {
        let keyed = Keyed()
        let key = MainWindowKey(isKeyWindow: { _ in keyed.value })
        let main = window()
        key.track(main)
        #expect(key.isKey == false)

        keyed.value = true
        post(NSWindow.didBecomeKeyNotification, main)
        #expect(key.isKey)
        #expect(key.isKeyNow)

        // A sheet or a second window takes the key state away.
        keyed.value = false
        post(NSWindow.didResignKeyNotification, main)
        #expect(key.isKey == false)
        #expect(key.isKeyNow == false)
    }

    @Test func anotherWindowsNotificationsChangeNothing() {
        let keyed = Keyed()
        let key = MainWindowKey(isKeyWindow: { _ in keyed.value })
        let main = window()
        let other = window()
        key.track(main)

        post(NSWindow.didBecomeKeyNotification, other)
        #expect(key.isKey == false)

        keyed.value = true
        post(NSWindow.didBecomeKeyNotification, main)
        post(NSWindow.didResignKeyNotification, other)
        post(NSWindow.willCloseNotification, other)
        #expect(key.isKey)
    }

    @Test func closingTheWindowEndsKeyState() {
        let keyed = Keyed()
        keyed.value = true
        let key = MainWindowKey(isKeyWindow: { _ in keyed.value })
        let main = window()
        key.track(main)
        #expect(key.isKey)

        post(NSWindow.willCloseNotification, main)
        #expect(key.isKey == false)
    }

    @Test func trackingTheSameWindowTwiceDoesNotDoubleTheObservers() {
        let keyed = Keyed()
        let key = MainWindowKey(isKeyWindow: { _ in keyed.value })
        let main = window()
        key.track(main)
        key.track(main)
        keyed.value = true
        post(NSWindow.didBecomeKeyNotification, main)
        #expect(key.isKey)
    }

    @Test func trackingANewWindowStopsFollowingTheOldOne() {
        let keyed = Keyed()
        let key = MainWindowKey(isKeyWindow: { _ in keyed.value })
        let first = window()
        let second = window()
        key.track(first)
        key.track(second)
        keyed.value = true
        post(NSWindow.didBecomeKeyNotification, first)
        #expect(key.isKey == false)
        post(NSWindow.didBecomeKeyNotification, second)
        #expect(key.isKey)
    }
}
