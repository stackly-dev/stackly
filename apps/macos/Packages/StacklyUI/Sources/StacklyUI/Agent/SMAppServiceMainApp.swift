import Foundation
import ServiceManagement

/// The live ``LoginItemControlling``: `SMAppService.mainApp` (spec 0032 AC-12). With
/// ``SMAppServiceAgent`` it is the only file that imports `ServiceManagement`.
/// Registering `mainApp` needs a signed app in `~/Applications` or `/Applications`; from any
/// other build the OS answers `notFound` or refuses, and the row says so.
nonisolated final class SMAppServiceMainApp: LoginItemControlling {
    /// Creates the controller.
    init() {}

    @concurrent func status() async -> LoginItemStatus {
        Self.map(SMAppService.mainApp.status)
    }

    @concurrent func register() async throws(LoginItemError) -> LoginItemStatus {
        do {
            try SMAppService.mainApp.register()
        } catch {
            throw Self.error(error, phrase: "add Stackly to Login Items")
        }
        return Self.map(SMAppService.mainApp.status)
    }

    @concurrent func unregister() async throws(LoginItemError) -> LoginItemStatus {
        do {
            try await SMAppService.mainApp.unregister()
        } catch {
            throw Self.error(error, phrase: "remove Stackly from Login Items")
        }
        return Self.map(SMAppService.mainApp.status)
    }

    func openLoginItems() {
        SMAppService.openSystemSettingsLoginItems()
    }

    /// The status the app uses for what the OS reports.
    static func map(_ status: SMAppService.Status) -> LoginItemStatus {
        switch status {
        case .enabled: .enabled
        case .requiresApproval: .requiresApproval
        case .notRegistered: .notRegistered
        case .notFound: .notFound
        @unknown default: .unrecognized(status.rawValue)
        }
    }

    private static func error(_ error: any Error, phrase: String) -> LoginItemError {
        let ns = error as NSError
        return LoginItemError(
            message: "macOS returned \(ns.domain) \(ns.code) while trying to \(phrase).")
    }
}
