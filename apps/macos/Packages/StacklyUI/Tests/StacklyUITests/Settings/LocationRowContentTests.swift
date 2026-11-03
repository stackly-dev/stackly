import Foundation
import StacklyClient
import Testing

@testable import StacklyUI

/// The location rows of the Storage tab (spec 0032 AC-19, AC-21, AC-24): the default folder,
/// Use Default, the older locations, "Not reachable", and the read only retention line.
struct LocationRowContentTests {
    private func row(
        _ kind: LocationKind, settings: DaemonSettingsView = FixtureSettings.view(),
        locations: [StorageLocationView] = FixtureSettings.locations, isConnected: Bool = true
    ) throws -> LocationRowContent {
        try #require(
            LocationRowContent.make(
                kind: kind, settings: settings, locations: locations, isConnected: isConnected))
    }

    @Test func theBuiltInDefaultShowsItsPathAndHidesUseDefault() throws {
        let data = try row(.data)
        #expect(data.title == "Instance data")
        #expect(data.path == FixtureSettings.dataLocation.path)
        #expect(data.usage == "Used for new instances")
        #expect(data.showsUseDefault == false)
        #expect(data.status == nil)
        #expect(data.older.isEmpty)
    }

    @Test func theCacheIsUsedForNewDownloads() throws {
        #expect(try row(.cache).usage == "Used for new downloads")
        #expect(try row(.cache).title == "Download cache")
    }

    @Test func aCustomDefaultOffersUseDefault() throws {
        let custom = FixtureSettings.location(
            .data, number: 9, path: "/Volumes/Work/Stackly", builtIn: false)
        let settings = FixtureSettings.view(data: custom)
        let data = try row(
            .data, settings: settings, locations: FixtureSettings.locations + [custom])
        #expect(data.showsUseDefault)
        #expect(data.path == "/Volumes/Work/Stackly")
    }

    @Test func anOlderDataLocationShowsItsInstancesAndAnEmptyOneStaysHidden() throws {
        let custom = FixtureSettings.location(
            .data, number: 9, path: "/Volumes/Work/Stackly", builtIn: false)
        let used = FixtureSettings.location(
            .data, number: 1, isDefault: false, instanceCount: 3)
        let settings = FixtureSettings.view(data: custom)
        let data = try row(.data, settings: settings, locations: [used, custom])
        #expect(data.older.map(\.usage) == ["Also used by 3 instances"])

        let empty = FixtureSettings.location(.data, number: 1, isDefault: false, instanceCount: 0)
        #expect(try row(.data, settings: settings, locations: [empty, custom]).older.isEmpty)

        let one = FixtureSettings.location(.data, number: 1, isDefault: false, instanceCount: 1)
        #expect(
            try row(.data, settings: settings, locations: [one, custom]).older.map(\.usage)
                == ["Also used by 1 instance"])
    }

    @Test func anOlderCacheLocationIsNoLongerUsedForNewDownloads() throws {
        let custom = FixtureSettings.location(
            .cache, number: 9, path: "/Volumes/Work/Stackly", builtIn: false)
        let old = FixtureSettings.location(.cache, number: 3, isDefault: false)
        let cache = try row(
            .cache, settings: FixtureSettings.view(cache: custom), locations: [old, custom])
        #expect(cache.older.map(\.usage) == ["No longer used for new downloads"])
    }

    @Test func aFolderThatIsGoneIsNotReachable() throws {
        let gone = FixtureSettings.location(.logs, number: 2, reachable: false)
        let logs = try row(.logs, settings: FixtureSettings.view(logs: gone))
        #expect(logs.status == "Not reachable")
    }

    @Test func offlineTheStatusIsUnknown() throws {
        let logs = try row(.logs, isConnected: false)
        #expect(logs.status == "Unknown while stacklyd is offline")
    }

    @Test func aKindThisBuildDoesNotKnowHasNoRow() {
        #expect(
            LocationRowContent.make(
                kind: .unrecognized, settings: FixtureSettings.view(),
                locations: FixtureSettings.locations, isConnected: true) == nil)
    }

    @Test func theConfirmationSaysWhatStaysWhereItIs() {
        #expect(
            LocationRowContent.confirmation(for: .data)
                == "Only instances you create from now on use this folder. Existing data stays where it is."
        )
        #expect(
            LocationRowContent.confirmation(for: .cache)
                == "New downloads go to this folder. Downloads already saved stay where they are.")
    }

    @Test func theRetentionLineUsesTheDaemonsNumbers() {
        let text = LocationRowContent.retentionText(
            LogRetention(segmentsPerStream: 10, segmentMaxBytes: 20 * 1024 * 1024))
        #expect(
            text
                == "Kept per stream: 10 files × 20 MiB. Older lines are removed as new ones arrive."
        )
    }
}
