import AppKit
import Foundation
import Observation

/// The choices that belong to this Mac and this app, not to the daemon (spec 0032 AC-14).
/// They live in `UserDefaults`, so they work while stacklyd is offline and stay when the app
/// starts again.
@Observable
public final class AppPreferences {
    /// How the app picks its light or dark look.
    public enum Appearance: String, CaseIterable, Identifiable, Sendable {
        /// Follow the Mac.
        case system
        /// Always light.
        case light
        /// Always dark.
        case dark

        /// The identity of the choice.
        public var id: String { rawValue }

        /// The label of the choice.
        public var title: String {
            switch self {
            case .system: "System"
            case .light: "Light"
            case .dark: "Dark"
            }
        }
    }

    /// The `UserDefaults` key of ``appearance``.
    public static let appearanceKey = "stackly.appearance"

    /// The `UserDefaults` key of ``notificationsEnabled``.
    public static let notificationsKey = "stackly.notificationsEnabled"

    /// Whether Stackly may send notifications (spec 0032 AC-22). Off until the person turns it
    /// on; macOS permission is a separate question the notification model asks.
    public var notificationsEnabled: Bool {
        didSet {
            guard notificationsEnabled != oldValue else { return }
            defaults.set(notificationsEnabled, forKey: Self.notificationsKey)
        }
    }

    /// The chosen appearance. Setting it saves it and applies it to every window of the app.
    public var appearance: Appearance {
        didSet {
            guard appearance != oldValue else { return }
            defaults.set(appearance.rawValue, forKey: Self.appearanceKey)
            apply(appearance)
        }
    }

    @ObservationIgnored private let defaults: UserDefaults
    @ObservationIgnored private let apply: (Appearance) -> Void

    /// Reads the saved choices and applies the appearance.
    /// - Parameters:
    ///   - defaults: Where the choices live; the app's own by default.
    ///   - apply: Puts an appearance on the app; `NSApp.appearance` by default.
    public init(
        defaults: UserDefaults = .standard,
        apply: @escaping (Appearance) -> Void = { AppPreferences.applyToApp($0) }
    ) {
        let saved = defaults.string(forKey: Self.appearanceKey).flatMap(Appearance.init)
        self.defaults = defaults
        self.apply = apply
        appearance = saved ?? .system
        notificationsEnabled = defaults.bool(forKey: Self.notificationsKey)
        apply(appearance)
    }

    /// Sets `NSApp.appearance`: `nil` follows the system, so every window and the popover
    /// change together.
    public static func applyToApp(_ appearance: Appearance) {
        switch appearance {
        case .system: NSApplication.shared.appearance = nil
        case .light: NSApplication.shared.appearance = NSAppearance(named: .aqua)
        case .dark: NSApplication.shared.appearance = NSAppearance(named: .darkAqua)
        }
    }
}
