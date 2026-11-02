import SwiftUI

/// The content of the Settings scene: one General tab with the Daemon section (spec 0030
/// AC-16). Feature 32 adds the other tabs and the other sections of General.
public struct SettingsRoot: View {
    /// The size of the Settings window.
    static let size = CGSize(width: 560, height: 260)

    private let model: DaemonModel

    /// Creates the Settings content.
    /// - Parameter model: The daemon model the app holds for the whole launch.
    public init(model: DaemonModel) {
        self.model = model
    }

    /// The Settings content.
    public var body: some View {
        TabView {
            Tab("General", systemImage: "gearshape") {
                GeneralSettingsTab()
            }
        }
        .environment(model)
        .environment(\.mutationAvailability, model.mutationAvailability)
        .frame(width: Self.size.width, height: Self.size.height)
    }
}

/// The General tab: for now the Daemon section only.
struct GeneralSettingsTab: View {
    @Environment(DaemonModel.self) private var model: DaemonModel?

    var body: some View {
        if let model {
            Form {
                DaemonSettingsSection(model: model)
            }
            .formStyle(.grouped)
        }
    }
}
