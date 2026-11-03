import Foundation
import Synchronization
import Testing

@testable import StacklyUI

/// Appearance, the one choice that belongs to the app (spec 0032 AC-14): System by default,
/// applied at once to the whole app, and saved in `UserDefaults`.
@Suite(.timeLimit(.minutes(1)))
struct AppPreferencesTests {
    /// A defaults store of its own, removed afterwards, and the appearances applied to the app.
    final class Rig: Sendable {
        let suite = "stackly.tests.\(UUID().uuidString)"
        let applied = Mutex<[AppPreferences.Appearance]>([])

        var defaults: UserDefaults { UserDefaults(suiteName: suite) ?? .standard }

        func preferences() -> AppPreferences {
            AppPreferences(
                defaults: defaults,
                apply: { [self] appearance in applied.withLock { $0.append(appearance) } })
        }

        deinit { UserDefaults().removePersistentDomain(forName: suite) }
    }

    @Test func appearanceStartsAtSystemAndAppliesIt() {
        let rig = Rig()
        let preferences = rig.preferences()
        #expect(preferences.appearance == .system)
        #expect(rig.applied.withLock { $0 } == [.system])
    }

    @Test func aChoiceAppliesAtOnceAndIsSaved() {
        let rig = Rig()
        let preferences = rig.preferences()
        preferences.appearance = .dark
        #expect(rig.applied.withLock { $0 } == [.system, .dark])
        #expect(rig.defaults.string(forKey: AppPreferences.appearanceKey) == "dark")
    }

    @Test func theChoiceSurvivesALaunch() {
        let rig = Rig()
        rig.preferences().appearance = .light
        let second = rig.preferences()
        #expect(second.appearance == .light)
        #expect(rig.applied.withLock { $0 }.last == .light)
    }

    @Test func theSameChoiceAgainChangesNothing() {
        let rig = Rig()
        let preferences = rig.preferences()
        preferences.appearance = .system
        #expect(rig.applied.withLock { $0 } == [.system])
    }

    @Test func aSavedValueThatIsNotAChoiceFallsBackToSystem() {
        let rig = Rig()
        rig.defaults.set("sepia", forKey: AppPreferences.appearanceKey)
        #expect(rig.preferences().appearance == .system)
    }

    @Test func notificationsAreOffUntilTheChoiceIsMadeAndTheChoiceIsSaved() {
        let rig = Rig()
        let preferences = rig.preferences()
        #expect(preferences.notificationsEnabled == false)
        preferences.notificationsEnabled = true
        #expect(rig.defaults.bool(forKey: AppPreferences.notificationsKey))
        #expect(rig.preferences().notificationsEnabled)
    }

    @Test func theChoicesAreSystemLightAndDarkInThatOrder() {
        #expect(AppPreferences.Appearance.allCases.map(\.title) == ["System", "Light", "Dark"])
    }
}
