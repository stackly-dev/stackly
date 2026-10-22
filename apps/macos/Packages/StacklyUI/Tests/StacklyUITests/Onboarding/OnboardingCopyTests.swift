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
            for token in ["MutationPath", "publishesCommands", "idempotencyKey", "ProcessInfo"] {
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
}
