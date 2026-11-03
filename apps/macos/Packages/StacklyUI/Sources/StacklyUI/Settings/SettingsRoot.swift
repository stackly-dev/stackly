import SwiftUI

/// The content of the Settings scene: General, Instances, Sources, Storage, and Updates
/// (spec 0032 AC-13).
public struct SettingsRoot: View {
    /// The size of the Settings window.
    static let size = CGSize(width: 560, height: 440)

    private let model: DaemonModel
    private let preferences: AppPreferences
    private let loginItem: LoginItemModel

    /// Creates the Settings content.
    /// - Parameters:
    ///   - model: The daemon model the app holds for the whole launch.
    ///   - preferences: The choices that belong to this Mac, such as Appearance.
    ///   - loginItem: Open Stackly at login, read from the OS.
    public init(model: DaemonModel, preferences: AppPreferences, loginItem: LoginItemModel) {
        self.model = model
        self.preferences = preferences
        self.loginItem = loginItem
    }

    /// The Settings content.
    public var body: some View {
        TabView {
            Tab(SettingsTab.general.title, systemImage: SettingsTab.general.symbol) {
                GeneralSettingsTab(loginItem: loginItem)
            }
            Tab(SettingsTab.instances.title, systemImage: SettingsTab.instances.symbol) {
                InstancesSettingsTab(model: model)
            }
            Tab(SettingsTab.sources.title, systemImage: SettingsTab.sources.symbol) {
                SourcesSettingsTab(model: model)
            }
            Tab(SettingsTab.storage.title, systemImage: SettingsTab.storage.symbol) {
                StorageSettingsTab(model: model)
            }
            Tab(SettingsTab.updates.title, systemImage: SettingsTab.updates.symbol) {
                UpdatesSettingsTab(model: model)
            }
        }
        .environment(model)
        .environment(preferences)
        .environment(\.mutationAvailability, model.mutationAvailability)
        .frame(width: Self.size.width, height: Self.size.height)
    }
}

/// The General tab: Appearance, the Startup group, and the Daemon section.
struct GeneralSettingsTab: View {
    let loginItem: LoginItemModel

    @Environment(DaemonModel.self) private var model: DaemonModel?
    @Environment(AppPreferences.self) private var preferences: AppPreferences?

    var body: some View {
        if let model, let preferences {
            Form {
                if SettingsAvailability.isShown(.appearance) {
                    Section("Appearance") {
                        Picker(
                            "Appearance",
                            selection: Binding(
                                get: { preferences.appearance },
                                set: { preferences.appearance = $0 })
                        ) {
                            ForEach(AppPreferences.Appearance.allCases) { appearance in
                                Text(appearance.title).tag(appearance)
                            }
                        }
                        .pickerStyle(.segmented)
                    }
                }
                StartupSettingsSection(model: model, loginItem: loginItem)
                DaemonSettingsSection(model: model)
            }
            .formStyle(.grouped)
        }
    }
}
