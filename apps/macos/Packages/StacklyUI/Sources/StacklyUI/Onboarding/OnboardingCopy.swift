import Foundation

/// Every piece of text the onboarding shows or announces (spec 0016, *Copy*). Views and
/// announcements read from here and write no text of their own, so the copy test and the
/// scan for forbidden phrases cover all of it.
nonisolated public enum OnboardingCopy {
    // MARK: - Steps and footer

    /// The label of each step, in order.
    static let stepLabels = [
        "Welcome", "Background Agent", "Storage", "Catalog Trust", "Command Line",
    ]

    /// The footer text of a step, such as `Step 2 of 5 · Background Agent`.
    static func stepText(number: Int, label: String) -> String {
        "Step \(number) of \(stepLabels.count) · \(label)"
    }

    /// What VoiceOver says for a step, and the announcement when the step changes.
    static func stepSpoken(number: Int, label: String) -> String {
        "Step \(number) of \(stepLabels.count), \(label)"
    }

    /// What VoiceOver announces once when the step changes.
    static func stepAnnouncement(number: Int, label: String) -> String {
        stepSpoken(number: number, label: label) + "."
    }

    /// The label of the Back button.
    static let back = "Back"
    /// The primary label for most steps.
    static let `continue` = "Continue"
    /// The primary label of the Background Agent step until the agent is allowed.
    static let notNow = "Not Now"
    /// The primary label of the last step.
    static let getStarted = "Get Started"
    /// The feedback after Get Started.
    static let setupComplete = "Setup complete."
    /// The Help menu item.
    static let helpMenuItem = "Welcome to Stackly"

    // MARK: - Welcome

    /// The title of the Welcome step.
    static let welcomeTitle = "Welcome to Stackly"
    /// The subtitle of the Welcome step.
    static let welcomeSubtitle =
        "Run local databases, queues and proxies side by side, each with its own data, port and secret."
    /// Whether the build installs runtimes through the daemon, which turns on the
    /// "Verified runtimes" row and the digest sentence (spec 0018, milestone 3).
    // provisional: F18
    static let runtimeInstallAvailable = false

    /// A feature row of the Welcome step.
    nonisolated struct Feature: Equatable, Sendable {
        /// The row title.
        let title: String
        /// The row detail.
        let detail: String
    }

    /// The feature rows of the Welcome step, without the runtime row.
    static let isolatedInstances = Feature(
        title: "Isolated instances",
        detail:
            "Each instance has its own data folder, port, and secret. Instances run as your user, not in a sandbox, so other apps you run can still read their files."
    )
    /// The Verified runtimes row.
    static let verifiedRuntimes = Feature(
        title: "Verified runtimes",
        detail: "Downloaded, checked against a pinned digest, then committed.")
    /// The No account needed row.
    static let noAccount = Feature(
        title: "No account needed",
        detail: "Stackly and stacklyd run on this Mac. Nothing here asks you to sign in.")

    /// The feature rows that show in this build, in order.
    static func features(runtimeInstallAvailable: Bool = Self.runtimeInstallAvailable)
        -> [Feature]
    {
        [isolatedInstances]
            + (runtimeInstallAvailable ? [verifiedRuntimes] : []) + [noAccount]
    }

    // MARK: - Background Agent, Catalog Trust, Command Line (titles until their steps)

    /// The title of the Background Agent step.
    static let agentTitle = "Keep services running in the background"
    /// The subtitle of the Background Agent step.
    static let agentSubtitle =
        "stacklyd supervises your instances when the window is closed. macOS asks you to allow it in Login Items."
    /// The title of the Catalog Trust step.
    static let catalogTitle = "Catalog sources"
    /// The subtitle of the Catalog Trust step.
    static let catalogSubtitle =
        "Providers describe how to install and run each server. Only trusted sources appear in the catalog."
    /// The title of the Command Line step.
    static let commandLineTitle = "Command line tool"
    /// The subtitle of the Command Line step.
    static let commandLineSubtitle =
        "The app and the CLI talk to the same daemon, so changes in one show up in the other."

    // MARK: - Storage

    /// The title of the Storage step.
    static let storageTitle = "Where your data lives"
    /// The subtitle of the Storage step.
    static let storageSubtitle =
        "Everything stays in your user Library. Instance data is kept by stable ID, so renaming never moves files."
    /// The subtitle of the Storage step while `STACKLY_ROOT` is set.
    static let storageSubtitleOverride =
        "Everything stays inside the STACKLY_ROOT folder. Instance data is kept by stable ID, so renaming never moves files."
    /// The label of the data row.
    static let storageData = "Data"
    /// The label of the cache row.
    static let storageCache = "Cache"
    /// The label of the logs row.
    static let storageLogs = "Logs"
    /// The label of the secrets row.
    static let storageSecrets = "Secrets"
    /// The label of the single folders row while `STACKLY_ROOT` is set.
    static let storageFolders = "Folders"
    /// The value of the secrets row.
    static let storageSecretsKeychain = "Login Keychain"
    /// The value of the secrets row while `STACKLY_ROOT` is set (spec 0011, AC-2).
    static let storageSecretsOverride = "A keychain file inside that folder"
    /// The text of the folders row while `STACKLY_ROOT` is set.
    static let storageOverrideNote =
        "STACKLY_ROOT is set, so stacklyd keeps data, cache, and logs inside that folder."
}
