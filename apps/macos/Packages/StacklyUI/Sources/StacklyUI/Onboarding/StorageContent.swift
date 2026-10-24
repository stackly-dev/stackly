import Foundation
import StacklyClient

/// What the Storage step shows (spec 0016, *Langkah Storage*). Pure, so each row of the
/// table is tested without a view.
nonisolated struct StorageContent: Equatable {
    /// One label and value of the list.
    struct Row: Equatable {
        /// The row label.
        let label: String
        /// The row value.
        let value: String
        /// Whether the value is a path, which takes the code style and can be selected.
        var isPath = false
    }

    /// The step subtitle.
    let subtitle: String
    /// The rows, in order.
    let rows: [Row]

    /// Builds the content.
    /// - Parameters:
    ///   - roots: The roots the last known daemon reported, or `nil` before any answered.
    ///   - home: The user's home folder, for the defaults and for writing `~`.
    ///   - rootOverride: Whether the app was launched with `STACKLY_ROOT`. Its value is never
    ///     passed in and never shown.
    /// - Returns: With the override, one Folders row and a keychain file for Secrets. Without
    ///   it, the daemon's folders when it reported its own, otherwise the macOS defaults, and
    ///   Secrets in the login Keychain.
    static func make(roots: DaemonRoots?, home: String, rootOverride: Bool) -> StorageContent {
        if rootOverride {
            return StorageContent(
                subtitle: OnboardingCopy.storageSubtitleOverride,
                rows: [
                    Row(
                        label: OnboardingCopy.storageFolders,
                        value: OnboardingCopy.storageOverrideNote),
                    Row(
                        label: OnboardingCopy.storageSecrets,
                        value: OnboardingCopy.storageSecretsOverride),
                ])
        }
        let paths: (data: String, cache: String, logs: String)
        if let roots, roots.rootOverride == false {
            paths = (roots.data, roots.cache, roots.logs)
        } else {
            let defaults = StorageLocations.defaults(home: home)
            paths = (defaults.data, defaults.cache, defaults.logs)
        }
        func shown(_ path: String) -> String { UserHome.tildePath(path, home: home) }
        return StorageContent(
            subtitle: OnboardingCopy.storageSubtitle,
            rows: [
                Row(label: OnboardingCopy.storageData, value: shown(paths.data), isPath: true),
                Row(label: OnboardingCopy.storageCache, value: shown(paths.cache), isPath: true),
                Row(label: OnboardingCopy.storageLogs, value: shown(paths.logs), isPath: true),
                Row(
                    label: OnboardingCopy.storageSecrets,
                    value: OnboardingCopy.storageSecretsKeychain),
            ])
    }
}
