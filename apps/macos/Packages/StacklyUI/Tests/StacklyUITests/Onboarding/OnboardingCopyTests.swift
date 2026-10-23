import Foundation
import Testing

@testable import StacklyUI

/// The onboarding text equals the *Copy* table of spec 0016, and the folder holds no phrase
/// that overstates isolation or trust, no network, and no way to a mutation (AC-4, AC-9).
struct OnboardingCopyTests {
    // MARK: - The Copy table (AC-9)

    @Test func theFooterAndStepTextsAreTheTable() {
        #expect(
            OnboardingStep.allCases.map(\.label)
                == ["Welcome", "Background Agent", "Storage", "Catalog Trust", "Command Line"])
        #expect(OnboardingCopy.back == "Back")
        #expect(OnboardingCopy.continue == "Continue")
        #expect(OnboardingCopy.notNow == "Not Now")
        #expect(OnboardingCopy.getStarted == "Get Started")
        #expect(OnboardingFooter.stepText(.storage) == "Step 3 of 5 · Storage")
        #expect(OnboardingFooter.announcement(.storage) == "Step 3 of 5, Storage.")
        #expect(OnboardingCopy.helpMenuItem == "Welcome to Stackly")
        #expect(OnboardingCopy.setupComplete == "Setup complete.")
    }

    @Test func theWelcomeTextsAreTheTable() {
        #expect(OnboardingCopy.welcomeTitle == "Welcome to Stackly")
        #expect(
            OnboardingCopy.welcomeSubtitle
                == "Run local databases, queues and proxies side by side, each with its own data, port and secret."
        )
        #expect(OnboardingCopy.isolatedInstances.title == "Isolated instances")
        #expect(
            OnboardingCopy.isolatedInstances.detail
                == "Each instance has its own data folder, port, and secret. Instances run as your user, not in a sandbox, so other apps you run can still read their files."
        )
        #expect(OnboardingCopy.verifiedRuntimes.title == "Verified runtimes")
        #expect(
            OnboardingCopy.verifiedRuntimes.detail
                == "Downloaded, checked against a pinned digest, then committed.")
        #expect(OnboardingCopy.noAccount.title == "No account needed")
        #expect(
            OnboardingCopy.noAccount.detail
                == "Stackly and stacklyd run on this Mac. Nothing here asks you to sign in.")
    }

    @Test func verifiedRuntimesShowsOnlyWhenTheBuildInstallsRuntimes() {
        #expect(
            OnboardingCopy.features(runtimeInstallAvailable: false).map(\.title)
                == ["Isolated instances", "No account needed"])
        #expect(
            OnboardingCopy.features(runtimeInstallAvailable: true).map(\.title)
                == ["Isolated instances", "Verified runtimes", "No account needed"])
        #expect(OnboardingCopy.runtimeInstallAvailable == false)
    }

    @Test func noEnvironmentsRowExistsYet() {
        let all = OnboardingCopy.features(runtimeInstallAvailable: true)
        #expect(all.contains { $0.title.contains("Environments") } == false)
    }

    @Test func theStorageAndStepTitlesAreTheTable() {
        #expect(OnboardingCopy.storageTitle == "Where your data lives")
        #expect(
            OnboardingCopy.storageSubtitle
                == "Everything stays in your user Library. Instance data is kept by stable ID, so renaming never moves files."
        )
        #expect(
            OnboardingCopy.storageSubtitleOverride
                == "Everything stays inside the STACKLY_ROOT folder. Instance data is kept by stable ID, so renaming never moves files."
        )
        #expect(
            [
                OnboardingCopy.storageData, OnboardingCopy.storageCache,
                OnboardingCopy.storageLogs, OnboardingCopy.storageSecrets,
                OnboardingCopy.storageFolders,
            ] == ["Data", "Cache", "Logs", "Secrets", "Folders"])
        #expect(OnboardingCopy.storageSecretsKeychain == "Login Keychain")
        #expect(OnboardingCopy.storageSecretsOverride == "A keychain file inside that folder")
        #expect(OnboardingCopy.agentTitle == "Keep services running in the background")
        #expect(OnboardingCopy.catalogTitle == "Catalog sources")
        #expect(OnboardingCopy.commandLineTitle == "Command line tool")
    }

    @Test func theStepSubtitlesAreTheTable() {
        #expect(
            OnboardingCopy.agentSubtitle
                == "stacklyd supervises your instances when the window is closed. macOS asks you to allow it in Login Items."
        )
        #expect(
            OnboardingCopy.catalogSubtitle
                == "Providers describe how to install and run each server. Only trusted sources appear in the catalog."
        )
        #expect(
            OnboardingCopy.commandLineSubtitle
                == "The app and the CLI talk to the same daemon, so changes in one show up in the other."
        )
    }

    @Test func theStorageOverrideRowIsTheTable() {
        #expect(
            OnboardingCopy.storageOverrideNote
                == "STACKLY_ROOT is set, so stacklyd keeps data, cache, and logs inside that folder."
        )
    }

    @Test func theStepAnnouncementsAreTheTable() {
        #expect(
            OnboardingCopy.stepText(number: 2, label: "Background Agent")
                == "Step 2 of 5 · Background Agent")
        #expect(
            OnboardingCopy.stepSpoken(number: 4, label: "Catalog Trust")
                == "Step 4 of 5, Catalog Trust")
        #expect(
            OnboardingCopy.stepAnnouncement(number: 5, label: "Command Line")
                == "Step 5 of 5, Command Line.")
    }

    // MARK: - The Background Agent card table (AC-9)

    @Test func theBackgroundAgentCardRowsAreTheTable() {
        #expect(OnboardingCopy.agentCheckingTitle == "Checking the background service…")
        #expect(OnboardingCopy.agentAllowedTitle == "Allowed in Login Items")
        #expect(OnboardingCopy.agentConnectedDetail == "stacklyd starts when you log in.")
        #expect(OnboardingCopy.agentStartingDetail == "stacklyd is starting…")
        #expect(OnboardingCopy.agentConnectingDetail == "Connecting to stacklyd…")
        #expect(OnboardingCopy.agentNotRunningTitle == "Allowed, but stacklyd isn’t running")
        #expect(
            OnboardingCopy.agentNotRunningDetail
                == "It was stopped or it exited. Start it again here, or run stackly daemon start in Terminal."
        )
        #expect(
            OnboardingCopy.agentCannotReachDetail
                == "Stackly can’t reach stacklyd right now. Diagnostics has the details after setup."
        )
        #expect(OnboardingCopy.agentOffTitle == "Background service is off")
        #expect(
            OnboardingCopy.agentOffDetail
                == "Turn it on so stacklyd can run your instances, even when this window is closed. macOS may ask you to allow it."
        )
        #expect(OnboardingCopy.agentApprovalTitle == "Waiting for approval")
        #expect(
            OnboardingCopy.agentApprovalDetail
                == "Turn on Stackly in System Settings > General > Login Items & Extensions. stacklyd can’t run until you do."
        )
        #expect(OnboardingCopy.agentDisabledTitle == "Turned off in Login Items")
        #expect(
            OnboardingCopy.agentDisabledDetail
                == "Turn on Stackly in System Settings > General > Login Items & Extensions to run stacklyd again."
        )
        #expect(OnboardingCopy.agentNotFoundTitle == "Stackly can’t find its background service")
        #expect(
            OnboardingCopy.agentNotFoundDetail
                == "macOS couldn’t find stacklyd for this copy of Stackly. Move Stackly to the Applications folder, or install it again."
        )
        #expect(OnboardingCopy.agentNoHelperTitle == "This build doesn’t include stacklyd")
        #expect(
            OnboardingCopy.agentNoHelperDetail
                == "Start stacklyd yourself, or build the app with cargo xtask build-app.")
        #expect(OnboardingCopy.agentRootOverrideTitle == "STACKLY_ROOT is set")
        #expect(
            OnboardingCopy.agentRootOverrideDetail
                == "The background service only runs the default Stackly folder. Start stacklyd yourself for this folder."
        )
        #expect(
            OnboardingCopy.agentUnavailableTitle
                == "Stackly couldn’t check its background service")
    }

    @Test func theUnavailableDetailNamesWhatTheModelKnowsOrSaysNothingMore() {
        #expect(
            OnboardingCopy.agentUnavailableDetail("timed out")
                == "macOS didn’t answer (timed out). Check again in a moment.")
        #expect(
            OnboardingCopy.agentUnavailableDetail("error 5")
                == "macOS didn’t answer (error 5). Check again in a moment.")
        #expect(
            OnboardingCopy.agentUnavailableDetail("unknown status 9")
                == "macOS didn’t answer (unknown status 9). Check again in a moment.")
        #expect(
            OnboardingCopy.agentUnavailableDetail(nil)
                == "macOS didn’t answer. Check again in a moment.")
    }

    @Test func theCardActionsAndTheWorkingReasonAreTheTable() {
        #expect(OnboardingCopy.actionTurnOn == "Turn On")
        #expect(OnboardingCopy.actionOpenSettings == "Open System Settings…")
        #expect(OnboardingCopy.actionStart == "Start stacklyd")
        #expect(OnboardingCopy.actionCheckAgain == "Check Again")
        #expect(OnboardingCopy.working == "Working…")
    }

    // MARK: - The Catalog Trust table (AC-9)

    @Test func theCatalogLabelsAndDetailsAreTheTable() {
        #expect(OnboardingCopy.catalogLabelPlain == "Catalog")
        #expect(OnboardingCopy.catalogLabelOfficial == "official")
        #expect(OnboardingCopy.catalogLabelDevelopment == "development")
        #expect(OnboardingCopy.catalogLabelUnknown == "Unknown source")
        #expect(
            OnboardingCopy.catalogDetailOfficial
                == "Maintained by the Stackly project and bundled with this app.")
        #expect(
            OnboardingCopy.catalogDetailDigest == " Runtimes are checked against pinned digests.")
        #expect(
            OnboardingCopy.catalogDetailDevelopment
                == "A development catalog from STACKLY_CATALOG_DIR. It isn’t the official catalog."
        )
        #expect(
            OnboardingCopy.catalogDetailUnknown
                == "This version of Stackly doesn’t recognize this catalog source.")
        #expect(OnboardingCopy.catalogDetailUnknownTrust == " Unknown trust.")
        #expect(
            OnboardingCopy.catalogDetailUnsupported
                == "This version of stacklyd doesn’t provide the catalog. Update Stackly.")
        #expect(
            OnboardingCopy.catalogDetailWaiting
                == "Stackly shows where its catalog comes from once stacklyd is running.")
        #expect(
            OnboardingCopy.catalogDetailError(message: "no route")
                == "The catalog isn’t available: no route.")
    }

    @Test func theCatalogRevisionLineAndTogglesAreTheTable() {
        #expect(
            OnboardingCopy.catalogRevision(revision: 7, published: "Oct 20, 2026", outdated: false)
                == "Revision 7 · published Oct 20, 2026")
        #expect(
            OnboardingCopy.catalogRevision(revision: 7, published: "Oct 20, 2026", outdated: true)
                == "Revision 7 · published Oct 20, 2026 · older than 180 days")
        #expect(OnboardingCopy.catalogToggleOfficial == "The official catalog is always on.")
        #expect(OnboardingCopy.catalogToggleDevelopment == "The development catalog is always on.")
        #expect(OnboardingCopy.catalogToggleUnknown == "This catalog is always on.")
        #expect(
            OnboardingCopy.catalogNote
                == "Trust tells you where a provider comes from. It isn’t a sandbox: every server runs with your user’s permissions."
        )
    }

    // MARK: - The Command Line table (AC-9)

    @Test func theCommandLineStatusTextsAreTheTable() {
        #expect(OnboardingCopy.commandLineRowTitle == "Install stackly in ~/.local/bin")
        #expect(
            OnboardingCopy.commandLineNotInstalled
                == "No password needed. Stackly doesn’t change your PATH or shell files.")
        #expect(
            OnboardingCopy.commandLineInstalled
                == "~/.local/bin/stackly points to this copy of Stackly.")
        #expect(
            OnboardingCopy.commandLineStale
                == "~/.local/bin/stackly points to another copy of Stackly.")
        #expect(
            OnboardingCopy.commandLineBroken
                == "~/.local/bin/stackly points to a copy of Stackly that’s gone.")
        #expect(
            OnboardingCopy.commandLineConflict
                == "~/.local/bin/stackly already exists and wasn’t made by Stackly. Stackly won’t change it."
        )
        #expect(OnboardingCopy.commandLineRepair == "Repair…")
    }

    @Test func thePathNoteAndTheDirectRunLeadInAreTheTable() {
        #expect(
            OnboardingCopy.commandLinePathNote
                == "If Terminal can’t find stackly, add this line to your shell profile:")
        #expect(OnboardingCopy.commandLinePathLine == #"export PATH="$HOME/.local/bin:$PATH""#)
        #expect(OnboardingCopy.commandLinePathWhat == "PATH line")
        #expect(OnboardingCopy.copied == "Copied.")
        #expect(OnboardingCopy.commandLineRunDirectly == "Or run it directly:")
    }

    @Test func theExampleBlockHoldsOnlyTheCommandsOfTheTableOneForEachLine() {
        #expect(OnboardingCopy.commandExamplesLabel == "Example commands")
        #expect(
            OnboardingCopy.commandExamples == ["stackly daemon status", "stackly provider list"])
        #expect(
            OnboardingCopy.commandExamplesText
                == "$ stackly daemon status\n$ stackly provider list")
    }

    // MARK: - The source rules over Onboarding/ (AC-4, AC-9, AC-3)

    /// The code of every Swift file in `Onboarding/`, with line comments cut off.
    private static func onboardingFiles() throws -> [SourceFile] {
        try SourceTree.swiftFiles().filter {
            $0.path.contains("/StacklyUI/Onboarding/")
        }
    }

    @Test func theFolderIsFoundAndHoldsTheCopyFile() throws {
        let paths = try Self.onboardingFiles().map(\.path)
        #expect(paths.contains { $0.hasSuffix("/Onboarding/OnboardingCopy.swift") })
    }

    /// Phrases the onboarding never says (spec 0016, AC-9): they overstate isolation or
    /// trust, or describe what the app does not do.
    static let forbiddenPhrases = [
        "never touching", "sandboxed", "can’t read", "can't read", "cannot read",
        "Signed manifests", "only while the app is open", "/usr/local/bin", "password once",
        "up to date", "latest",
    ]

    @Test func noFileOfTheFolderSaysAForbiddenPhrase() throws {
        for file in try Self.onboardingFiles() {
            let code = file.codeLines.joined(separator: "\n")
            for phrase in Self.forbiddenPhrases {
                #expect(
                    code.localizedCaseInsensitiveContains(phrase) == false,
                    "\(file.path) says “\(phrase)”")
            }
        }
    }

    @Test func theForbiddenPhraseScanFindsAPhraseInAString() {
        let text = #"let a = Text("Your catalog is up to date")"#
        let code = SourceFile(path: "a", text: text).codeLines.joined(separator: "\n")
        #expect(code.localizedCaseInsensitiveContains("up to date"))
    }

    @Test func theFolderUsesNoNetwork() throws {
        for file in try Self.onboardingFiles() {
            let code = file.codeLines.joined(separator: "\n")
            for token in ["URLSession", "http://", "https://", "NWConnection", "URLRequest"] {
                #expect(code.contains(token) == false, "\(file.path) uses \(token)")
            }
        }
    }

    @Test func theFolderHasNoWayToAMutationOrACommandOfTheShell() throws {
        for file in try Self.onboardingFiles() {
            let code = file.codeLines.joined(separator: "\n")
            for token in [
                "MutationPath", "publishesCommands", "idempotencyKey", "ProcessInfo",
                ".register(", ".unregister(", ".relaunch(",
            ] {
                #expect(code.contains(token) == false, "\(file.path) uses \(token)")
            }
        }
    }

    @Test func onlyTheFlowAnnouncesAndNeverSetupComplete() throws {
        for file in try Self.onboardingFiles() {
            let code = file.codeLines.joined(separator: "\n")
            if file.path.hasSuffix("/OnboardingFlow.swift") { continue }
            #expect(code.contains("announce(") == false, "\(file.path) announces")
        }
        for file in try Self.onboardingFiles() {
            let code = file.codeLines.joined(separator: "\n")
            #expect(code.contains(#"announce("Setup complete.")"#) == false)
        }
    }

    // MARK: - One file holds the text (AC-9)

    /// A string literal of English prose: it starts with a capital letter and holds a space.
    /// Paths and identifiers do not match.
    private static let prose = /"[A-Z][^"\\]* [^"\\]*"/

    @Test func everyPhraseOfTheOnboardingLivesInTheCopyFile() throws {
        for file in try Self.onboardingFiles()
        where file.path.hasSuffix("/OnboardingCopy.swift") == false {
            // The names of previews are for the person who reads the canvas, not UI text.
            for line in file.codeLines
            where line.contains("#Preview(") == false && line.firstMatch(of: Self.prose) != nil {
                Issue.record("\(file.path) holds text of its own: \(line)")
            }
        }
    }

    @Test func theProseScanFindsASentenceAndSkipsPathsAndIdentifiers() {
        #expect(#"Text("Turn it on so stacklyd runs")"#.firstMatch(of: Self.prose) != nil)
        #expect(
            #"let a = "/Library/Application Support/Stackly""#.firstMatch(of: Self.prose) == nil)
        #expect(#"let b = "onboarding.finished""#.firstMatch(of: Self.prose) == nil)
        #expect(#"let c = "keyboard""#.firstMatch(of: Self.prose) == nil)
    }
}
