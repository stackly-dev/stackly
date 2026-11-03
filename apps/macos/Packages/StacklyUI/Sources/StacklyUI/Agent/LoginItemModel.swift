import Foundation
import Observation

/// The state behind the Open Stackly at login toggle (spec 0032 AC-12). The state is always
/// read from the OS, never kept as a setting of its own: the toggle follows what the user does
/// in System Settings, and it is separate from Start stacklyd at login, so turning it off
/// touches neither the daemon nor any instance.
@Observable
public final class LoginItemModel {
    /// What the OS last said, or `nil` before the first answer.
    public private(set) var status: LoginItemStatus?
    /// Whether a register or unregister call is out.
    public private(set) var isWorking = false
    /// The sentence for the last call that failed, or `nil`.
    public private(set) var error: String?

    @ObservationIgnored private let controller: any LoginItemControlling

    /// Creates the model over the live `SMAppService.mainApp`.
    public convenience init() {
        self.init(controller: SMAppServiceMainApp())
    }

    init(controller: any LoginItemControlling) {
        self.controller = controller
    }

    /// Whether the toggle shows on: the app is registered, whether or not the user has
    /// allowed it yet.
    public var isOn: Bool {
        switch status {
        case .enabled, .requiresApproval: true
        default: false
        }
    }

    /// Whether the row offers Open Login Items.
    public var needsApproval: Bool { status == .requiresApproval }

    /// Reads the state again. The app calls it when it becomes active and when Settings opens.
    public func refresh() async {
        status = await controller.status()
    }

    /// Registers or unregisters the app, then shows what the OS says. A failure puts the
    /// toggle back to what the OS reports and keeps the sentence in ``error``.
    /// - Parameter enabled: Whether the app should open at login.
    public func setEnabled(_ enabled: Bool) async {
        guard isWorking == false else { return }
        isWorking = true
        error = nil
        defer { isWorking = false }
        do {
            status = enabled ? try await controller.register() : try await controller.unregister()
        } catch {
            self.error = error.message
            status = await controller.status()
        }
    }

    /// Opens Login Items in System Settings.
    public func openLoginItems() {
        controller.openLoginItems()
    }

    /// A model over a fake, for tests and fixtures.
    #if DEBUG
        static func fake(status: LoginItemStatus = .notRegistered) -> LoginItemModel {
            LoginItemModel(controller: FakeLoginItem(status: status))
        }
    #endif
}
