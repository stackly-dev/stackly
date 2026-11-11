import Foundation
import StacklyClient
import Testing

@testable import StacklyUI

/// The table of rows (spec 0032 AC-13) and the words and rule shared by the tabs (AC-23,
/// AC-24).
struct SettingsAvailabilityTests {
    @Test func everyRowHasExactlyOneEntry() {
        let rows = SettingsAvailability.entries.map(\.row)
        #expect(Set(rows).count == rows.count)
        #expect(Set(rows) == Set(SettingsRow.allCases))
    }

    @Test func theRowsOfFeaturesNotBuiltStayOut() {
        #expect(SettingsAvailability.isShown(.cleanUpCache) == false)
        #expect(SettingsAvailability.isShown(.communitySources) == false)
        let community = SettingsAvailability.entries.first { $0.row == .communitySources }
        #expect(community?.owner == "Deferred (public registry)")
    }

    @Test func theImportedProvidersGroupIsBuiltByFeature37() {
        let entry = SettingsAvailability.entries.first { $0.row == .importProviders }
        #expect(entry?.isBuilt == true)
        #expect(entry?.owner == "feature 37")
        #expect(entry?.tab == .sources)
    }

    @Test func theRowsOfThisMilestoneShow() {
        #expect(SettingsAvailability.rows(in: .updates) == [.updateChannel, .telemetry])
        #expect(
            SettingsAvailability.rows(in: .instances) == [.autostartNewInstances, .notifications])
        #expect(
            SettingsAvailability.rows(in: .sources) == [
                .catalogSource, .providers, .importProviders,
            ])
        #expect(
            SettingsAvailability.rows(in: .storage)
                == [.dataLocation, .logsLocation, .cacheLocation, .logRetention, .auditRetention])
        #expect(SettingsAvailability.rows(in: .general).contains(.appearance))
    }

    @Test func theTabsAreInTheOrderOfTheSpec() {
        #expect(
            SettingsTab.allCases.map(\.title)
                == ["General", "Instances", "Sources", "Storage", "Updates"])
    }

    // MARK: - Offline

    @Test func aRowIsLiveWhileTheDaemonAnswers() {
        #expect(SettingsCopy.mode(isConnected: true, hasLoaded: false) == .live)
        #expect(SettingsCopy.mode(isConnected: true, hasLoaded: true) == .live)
    }

    @Test func aRowShowsTheLastKnownValueWhenOfflineAfterARead() {
        #expect(SettingsCopy.mode(isConnected: false, hasLoaded: true) == .lastKnown)
    }

    @Test func aRowIsUnavailableWhenOfflineAndNeverRead() {
        #expect(SettingsCopy.mode(isConnected: false, hasLoaded: false) == .unavailable)
    }

    @Test func theNotesSayWhatTheChoiceDoesToday() {
        #expect(
            SettingsCopy.telemetryNote
                == "Stackly doesn’t collect usage statistics yet. Your choice is saved for when it does. Diagnostic bundles are never uploaded automatically."
        )
        #expect(
            SettingsCopy.channelNote
                == "Saved now. Used once Stackly can update itself. App updates never touch instance data."
        )
        #expect(SettingsCopy.offlineTitle == "Can’t connect to stacklyd.")
        #expect(SettingsCopy.offlineMessage == "Showing the last known settings.")
        #expect(SettingsCopy.notAvailable == "Not available while stacklyd is offline.")
        #expect(SettingsCopy.changedElsewhere == "Changed elsewhere. Try again.")
    }

    @Test func theActivityRowSaysHowLongAndHowManyEntriesAreKept() {
        #expect(SettingsCopy.auditRetentionTitle == "Keep activity for")
        #expect(
            SettingsCopy.auditRetentionChoices.map(SettingsCopy.days) == [
                "7 days", "30 days", "90 days",
            ])
        #expect(
            SettingsCopy.auditRetentionNote(maxRows: 100_000, locale: Locale(identifier: "en_US"))
                == "Up to 100,000 entries. Older entries are removed every day and right after you change this."
        )
        #expect(
            SettingsCopy.auditRetentionRemoveTitle(days: 7) == "Remove activity older than 7 days?")
        #expect(
            SettingsCopy.auditRetentionRemoveMessage
                == "Older entries are removed now and can’t be brought back.")
    }

    @Test func aCatalogSourceReadsAsAPlace() {
        #expect(SettingsCopy.sourceText(.bundled) == "Bundled with Stackly")
        #expect(SettingsCopy.sourceText(.development) == "Development catalog folder")
        #expect(SettingsCopy.sourceText(.unrecognized) == "Unknown source")
    }
}
