import Foundation
import StacklyClient

/// Where the safe action of a finding goes (spec 0034, AC-18): a page of the window, the
/// endpoints sheet with the suggested port filled in, which changes nothing until Apply, or
/// the Storage tab of Settings. No route removes a lock, a file, or data, or stops a process.
struct DoctorRouter {
    /// The navigator of the window, or `nil` in a preview.
    let navigator: ShellNavigator?
    /// The tab of the Settings window, or `nil` without a daemon model.
    let settingsTab: SettingsTabSelection?
    /// Opens the Settings window.
    let openSettings: @MainActor () -> Void

    /// Goes where `route` says; `finding` gives the instance name the endpoints sheet shows.
    func open(_ route: DoctorActionRoute, finding: DoctorFinding?) {
        switch route {
        case .showInstance(let id):
            navigator?.openInstance(id)
        case .reviewConfig(let id):
            navigator?.openInstance(id, tab: .configuration)
        case .changePort(let id, let endpoint, let port):
            let source = InstanceEditSource(id: id, name: finding?.instance?.name ?? "")
            navigator?.openInstance(
                id, tab: .configuration, edit: .editPorts(source, prefill: [endpoint: port]))
        case .showOperation(let id):
            navigator?.openOperation(id)
        case .openStorageSettings:
            settingsTab?.tab = .storage
            openSettings()
        }
    }
}
