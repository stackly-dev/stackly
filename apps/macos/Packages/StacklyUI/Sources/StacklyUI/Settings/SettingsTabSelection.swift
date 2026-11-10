import Observation

/// The tab the Settings window shows. ``DaemonModel`` holds one for the launch, so another
/// window can choose a tab before it opens Settings, such as Open Settings of a doctor
/// finding, which shows Storage (spec 0034, AC-18).
@Observable
final class SettingsTabSelection {
    /// The tab that shows.
    var tab: SettingsTab = .general
}
