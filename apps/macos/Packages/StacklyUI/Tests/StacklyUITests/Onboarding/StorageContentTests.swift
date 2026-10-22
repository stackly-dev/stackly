import StacklyClient
import Testing

@testable import StacklyUI

/// The Storage step: the daemon's folders, the macOS defaults, and the override variant
/// (spec 0016, AC-6).
struct StorageContentTests {
    private static func roots(
        data: String = "/Users/a/Library/Application Support/Stackly",
        cache: String = "/Users/a/Library/Caches/Stackly",
        logs: String = "/Users/a/Library/Logs/Stackly",
        rootOverride: Bool = false
    ) -> DaemonRoots {
        DaemonRoots(
            data: data, logs: logs, cache: cache, runtime: "/var/folders/x/stackly",
            rootOverride: rootOverride)
    }

    private static func values(_ content: StorageContent) -> [String: String] {
        Dictionary(uniqueKeysWithValues: content.rows.map { ($0.label, $0.value) })
    }

    @Test func theDefaultsFollowTheMacOSTableOfSpec0004() {
        let folders = StorageLocations.defaults(home: "/Users/a")
        #expect(folders.data == "/Users/a/Library/Application Support/Stackly")
        #expect(folders.cache == "/Users/a/Library/Caches/Stackly")
        #expect(folders.logs == "/Users/a/Library/Logs/Stackly")
    }

    @Test func aTrailingSlashOfTheHomeDoesNotDoubleTheSeparator() {
        #expect(
            StorageLocations.defaults(home: "/Users/a/").data
                == "/Users/a/Library/Application Support/Stackly")
    }

    @Test func withoutADaemonTheRowsAreTheDefaultsWrittenWithATilde() {
        let content = StorageContent.make(roots: nil, home: "/Users/a", rootOverride: false)
        #expect(content.subtitle == OnboardingCopy.storageSubtitle)
        #expect(
            content.rows == [
                .init(label: "Data", value: "~/Library/Application Support/Stackly", isPath: true),
                .init(label: "Cache", value: "~/Library/Caches/Stackly", isPath: true),
                .init(label: "Logs", value: "~/Library/Logs/Stackly", isPath: true),
                .init(label: "Secrets", value: "Login Keychain"),
            ])
    }

    @Test func theRowsAreTheFoldersTheLastKnownDaemonReported() {
        let reported = Self.roots(
            data: "/Users/a/Library/Application Support/Stackly",
            cache: "/Volumes/Fast/Caches/Stackly", logs: "/Users/a/Library/Logs/Stackly")
        let values = Self.values(
            StorageContent.make(roots: reported, home: "/Users/a", rootOverride: false))
        #expect(values["Data"] == "~/Library/Application Support/Stackly")
        #expect(values["Cache"] == "/Volumes/Fast/Caches/Stackly")
        #expect(values["Logs"] == "~/Library/Logs/Stackly")
        #expect(values["Secrets"] == "Login Keychain")
    }

    @Test func aDaemonThatRanWithAnOverrideGivesWayToTheDefaults() {
        let other = Self.roots(
            data: "/tmp/test/data", cache: "/tmp/test/cache", logs: "/tmp/test/logs",
            rootOverride: true)
        let content = StorageContent.make(roots: other, home: "/Users/a", rootOverride: false)
        #expect(Self.values(content)["Data"] == "~/Library/Application Support/Stackly")
        #expect(Self.values(content)["Secrets"] == "Login Keychain")
    }

    @Test func anActiveOverrideShowsOneFolderNoteAndAKeychainFile() {
        let content = StorageContent.make(
            roots: Self.roots(), home: "/Users/a", rootOverride: true)
        #expect(content.subtitle == OnboardingCopy.storageSubtitleOverride)
        #expect(
            content.rows == [
                .init(
                    label: "Folders",
                    value:
                        "STACKLY_ROOT is set, so stacklyd keeps data, cache, and logs inside that folder."
                ),
                .init(label: "Secrets", value: "A keychain file inside that folder"),
            ])
    }

    @Test func aPathOfAnotherUserWhoseNameStartsWithTheHomeStaysWhole() {
        let reported = Self.roots(data: "/Users/ab/x", cache: "/Users/a", logs: "/Users/a/l")
        let values = Self.values(
            StorageContent.make(roots: reported, home: "/Users/a", rootOverride: false))
        #expect(values["Data"] == "/Users/ab/x")
        #expect(values["Cache"] == "~")
        #expect(values["Logs"] == "~/l")
    }

    @Test func theOverrideValueNeverReachesTheContent() {
        let content = StorageContent.make(
            roots: Self.roots(data: "/secret/override/root"), home: "/Users/a",
            rootOverride: true)
        let text = content.rows.map(\.value).joined() + content.subtitle
        #expect(text.contains("/secret/override/root") == false)
    }
}
