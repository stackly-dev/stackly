import Foundation

/// The visible text of the onboarding flow.
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

// MARK: - Background Agent card (spec 0016, *Kartu Background Agent*)

nonisolated extension OnboardingCopy {
    /// The title of an enabled row.
    static let agentAllowedTitle = "Allowed in Login Items"
    /// The title of the row while the status is read.
    static let agentCheckingTitle = "Checking the background service…"
    /// The detail of the connected row.
    static let agentConnectedDetail = "stacklyd starts when you log in."
    /// The detail of the row while stacklyd starts.
    static let agentStartingDetail = "stacklyd is starting…"
    /// The title of the row when stacklyd is missing for 15 seconds or more.
    static let agentNotRunningTitle = "Allowed, but stacklyd isn’t running"
    /// The detail of that row.
    static let agentNotRunningDetail =
        "It was stopped or it exited. Start it again here, or run stackly daemon start in Terminal."
    /// The detail of the row while the link connects.
    static let agentConnectingDetail = "Connecting to stacklyd…"
    /// The detail of an enabled row in any other presence.
    static let agentCannotReachDetail =
        "Stackly can’t reach stacklyd right now. Diagnostics has the details after setup."
    /// The title of the row of an agent that is off.
    static let agentOffTitle = "Background service is off"
    /// The detail of that row.
    static let agentOffDetail =
        "Turn it on so stacklyd can run your instances, even when this window is closed. macOS may ask you to allow it."
    /// The title of the row that waits for approval.
    static let agentApprovalTitle = "Waiting for approval"
    /// The detail of that row.
    static let agentApprovalDetail =
        "Turn on Stackly in System Settings > General > Login Items & Extensions. stacklyd can’t run until you do."
    /// The title of the row of an agent the user turned off.
    static let agentDisabledTitle = "Turned off in Login Items"
    /// The detail of that row.
    static let agentDisabledDetail =
        "Turn on Stackly in System Settings > General > Login Items & Extensions to run stacklyd again."
    /// The title of the row of a service macOS cannot find.
    static let agentNotFoundTitle = "Stackly can’t find its background service"
    /// The detail of that row.
    static let agentNotFoundDetail =
        "macOS couldn’t find stacklyd for this copy of Stackly. Move Stackly to the Applications folder, or install it again."
    /// The title of the row of a build without stacklyd.
    static let agentNoHelperTitle = "This build doesn’t include stacklyd"
    /// The detail of that row.
    static let agentNoHelperDetail =
        "Start stacklyd yourself, or build the app with cargo xtask build-app."
    /// The title of the row while `STACKLY_ROOT` is set.
    static let agentRootOverrideTitle = "STACKLY_ROOT is set"
    /// The detail of that row.
    static let agentRootOverrideDetail =
        "The background service only runs the default Stackly folder. Start stacklyd yourself for this folder."
    /// The title of the row when the status cannot be read.
    static let agentUnavailableTitle = "Stackly couldn’t check its background service"

    /// The detail of that row.
    /// - Parameter detail: What the model knows, such as "timed out", or `nil`.
    static func agentUnavailableDetail(_ detail: String?) -> String {
        detail.map { "macOS didn’t answer (\($0)). Check again in a moment." }
            ?? "macOS didn’t answer. Check again in a moment."
    }

    /// The reason the card action is disabled while an action runs.
    static let working = "Working…"
    /// The action that turns the agent on.
    static let actionTurnOn = "Turn On"
    /// The action that opens Login Items.
    static let actionOpenSettings = "Open System Settings…"
    /// The action that starts stacklyd.
    static let actionStart = "Start stacklyd"
    /// The action that reads the status again.
    static let actionCheckAgain = "Check Again"
}

// MARK: - Command Line step (spec 0016, *Kontrol Command Line*)

nonisolated extension OnboardingCopy {
    /// The row title of the Command Line step.
    static let commandLineRowTitle = "Install stackly in ~/.local/bin"
    /// The detail while nothing is installed.
    static let commandLineNotInstalled =
        "No password needed. Stackly doesn’t change your PATH or shell files."
    /// The detail while the link points to this copy.
    static let commandLineInstalled = "~/.local/bin/stackly points to this copy of Stackly."
    /// The detail while the link points to another copy.
    static let commandLineStale = "~/.local/bin/stackly points to another copy of Stackly."
    /// The detail while the link points to a copy that is gone.
    static let commandLineBroken =
        "~/.local/bin/stackly points to a copy of Stackly that’s gone."
    /// The detail, and the reason the toggle is off, while something else is there.
    static let commandLineConflict =
        "~/.local/bin/stackly already exists and wasn’t made by Stackly. Stackly won’t change it."
    /// The title of the Repair button.
    static let commandLineRepair = "Repair…"
    /// What introduces the PATH line.
    static let commandLinePathNote =
        "If Terminal can’t find stackly, add this line to your shell profile:"
    /// The line to add to a shell profile.
    static let commandLinePathLine = #"export PATH="$HOME/.local/bin:$PATH""#
    /// What the Copy button of the PATH line names.
    static let commandLinePathWhat = "PATH line"
    static let copied = "Copied."
    /// What introduces the path of the command inside the app.
    static let commandLineRunDirectly = "Or run it directly:"
    static let commandExamplesLabel = "Example commands"
    /// The commands of the example block; commands of later features join with them
    /// (spec 0016, *Fitur yang belum ada*).
    static let commandExamples = ["stackly daemon status", "stackly provider list"]
    /// The example block as it shows: one command per line, each after `$ `.
    static var commandExamplesText: String {
        commandExamples.map { "$ " + $0 }.joined(separator: "\n")
    }
}

// MARK: - Catalog Trust

nonisolated extension OnboardingCopy {
    /// The label of the row until the catalog is known, and when it failed or is unsupported.
    static let catalogLabelPlain = "Catalog"
    /// The label of the official catalog.
    static let catalogLabelOfficial = "official"
    /// The label of a development catalog.
    static let catalogLabelDevelopment = "development"
    /// The label of a catalog source or trust this build does not know.
    static let catalogLabelUnknown = "Unknown source"

    /// The detail of the official catalog.
    static let catalogDetailOfficial =
        "Maintained by the Stackly project and bundled with this app."
    /// The sentence added to the official detail when the build installs runtimes.
    static let catalogDetailDigest = " Runtimes are checked against pinned digests."
    /// The detail of a development catalog.
    static let catalogDetailDevelopment =
        "A development catalog from STACKLY_CATALOG_DIR. It isn’t the official catalog."
    /// The detail of a catalog source or trust this build does not know.
    static let catalogDetailUnknown =
        "This version of Stackly doesn’t recognize this catalog source."
    /// The sentence added to the unknown detail when only the trust is unknown.
    static let catalogDetailUnknownTrust = " Unknown trust."
    /// The detail when the daemon does not provide the catalog.
    static let catalogDetailUnsupported =
        "This version of stacklyd doesn’t provide the catalog. Update Stackly."
    /// The detail before the daemon answered.
    static let catalogDetailWaiting =
        "Stackly shows where its catalog comes from once stacklyd is running."

    /// The detail when the catalog failed to load.
    static func catalogDetailError(message: String) -> String {
        "The catalog isn’t available: \(message)."
    }

    /// The revision line: `Revision 7 · published Oct 20, 2026`.
    static func catalogRevision(revision: UInt64, published: String, outdated: Bool) -> String {
        let line = "Revision \(revision) · published \(published)"
        return outdated ? line + catalogOutdatedSuffix : line
    }

    /// The suffix of the revision line of a catalog the daemon marks outdated.
    static let catalogOutdatedSuffix = " · older than 180 days"

    /// The label and help of the toggle of the official catalog.
    static let catalogToggleOfficial = "The official catalog is always on."
    /// The label and help of the toggle of a development catalog.
    static let catalogToggleDevelopment = "The development catalog is always on."
    /// The label and help of the toggle of a catalog this build does not know.
    static let catalogToggleUnknown = "This catalog is always on."

    /// The note under the row.
    static let catalogNote =
        "Trust tells you where a provider comes from. It isn’t a sandbox: every server runs with your user’s permissions."
}
