import Foundation

/// The one thing the onboarding remembers: whether it is finished (spec 0016, *Model
/// data*). It is a GUI preference, never kept in the daemon, the registry, or the data root.
public protocol OnboardingRecordStore: AnyObject {
    /// Whether Get Started or Escape closed the onboarding earlier.
    var isFinished: Bool { get }

    /// Records that the onboarding is finished.
    func markFinished()
}

/// The store of the running app: the `onboarding.finished` key of the app's `UserDefaults`.
/// Launch with `-onboarding.finished NO` to see the onboarding again without deleting
/// anything.
public final class UserDefaultsOnboardingRecordStore: OnboardingRecordStore {
    /// The `UserDefaults` key.
    public static let key = "onboarding.finished"

    private let defaults: UserDefaults

    /// Creates a store.
    /// - Parameter defaults: The defaults to use, the app's standard ones by default.
    public init(defaults: UserDefaults = .standard) {
        self.defaults = defaults
    }

    /// Whether the key holds `true`. A missing key means `false`.
    public var isFinished: Bool {
        defaults.bool(forKey: Self.key)
    }

    /// Writes `true`.
    public func markFinished() {
        defaults.set(true, forKey: Self.key)
    }
}

/// A store that keeps the value in memory, for tests, previews, and the fixture runtime.
public final class InMemoryOnboardingRecordStore: OnboardingRecordStore {
    /// Whether the onboarding is finished.
    public private(set) var isFinished: Bool

    /// How many times ``markFinished()`` ran.
    public private(set) var writes = 0

    /// Creates a store.
    /// - Parameter isFinished: The value to start with.
    public init(isFinished: Bool = false) {
        self.isFinished = isFinished
    }

    /// Records that the onboarding is finished.
    public func markFinished() {
        isFinished = true
        writes += 1
    }
}
