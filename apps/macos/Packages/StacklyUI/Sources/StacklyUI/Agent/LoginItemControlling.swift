import Foundation

/// What the OS says about Stackly itself as a login item (spec 0032 AC-12).
nonisolated public enum LoginItemStatus: Sendable, Equatable {
    /// Registered and allowed: Stackly opens at login.
    case enabled
    /// Registered, and the user still has to allow it in Login Items.
    case requiresApproval
    /// Not registered.
    case notRegistered
    /// The OS cannot find the app, such as a build that does not run from an app bundle.
    case notFound
    /// A status value this app does not know.
    case unrecognized(Int)
}

/// Why a login item call failed, in a sentence for the Settings row.
nonisolated public struct LoginItemError: Error, Sendable, Equatable {
    /// The sentence the row shows.
    public let message: String

    /// Creates an error.
    /// - Parameter message: The sentence the row shows.
    public init(message: String) {
        self.message = message
    }
}

/// The OS side of Open Stackly at login (spec 0032 AC-12). The live one is
/// ``SMAppServiceMainApp``, the only file that imports `ServiceManagement` for the app itself;
/// tests, previews, and fixtures use ``FakeLoginItem``.
nonisolated protocol LoginItemControlling: Sendable {
    /// Reads the registration state. It never prompts and never changes the registration.
    @concurrent func status() async -> LoginItemStatus
    /// Registers the app as a login item and reads the state again.
    @concurrent func register() async throws(LoginItemError) -> LoginItemStatus
    /// Removes the app from the login items and reads the state again.
    @concurrent func unregister() async throws(LoginItemError) -> LoginItemStatus
    /// Opens Login Items in System Settings.
    func openLoginItems()
}
