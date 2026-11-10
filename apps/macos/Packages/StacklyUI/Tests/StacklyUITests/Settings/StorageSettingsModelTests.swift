import Foundation
import StacklyClient
import Synchronization
import Testing

@testable import StacklyUI

/// Change…, Use Default, and Reveal in Finder (spec 0032 AC-19): the folder panel comes first,
/// then the confirmation, and only the confirmation sends the change.
@Suite(.timeLimit(.minutes(1)))
struct StorageSettingsModelTests {
    /// A picker that answers a fixed path, or cancels.
    struct FixedPicker: FolderPicking {
        let path: String?

        func chooseFolder(message: String, startingAt: String?) async -> String? { path }
    }

    private func rig(picking path: String?) async -> (
        StorageSettingsModel, SettingsStoreTests.Daemon
    ) {
        let daemon = SettingsStoreTests.Daemon()
        let store = SettingsStoreTests.store(daemon)
        await store.refresh()
        return (
            StorageSettingsModel(store: store, picker: FixedPicker(path: path), reveal: { _ in }),
            daemon
        )
    }

    private func auditRig(days: UInt32) async -> (
        StorageSettingsModel, SettingsStore, SettingsStoreTests.Daemon
    ) {
        let daemon = SettingsStoreTests.Daemon(FixtureSettings.view(auditDays: days))
        let store = SettingsStoreTests.store(daemon)
        await store.refresh()
        return (
            StorageSettingsModel(store: store, picker: FixedPicker(path: nil), reveal: { _ in }),
            store, daemon
        )
    }

    private func sentUpdates(_ daemon: SettingsStoreTests.Daemon) -> [Call] {
        daemon.calls.filter {
            if case .settingsUpdate = $0 { return true } else { return false }
        }
    }

    private static func auditPatch(_ days: UInt32, revision: UInt64 = 3) -> Call {
        .settingsUpdate(
            UpdateSettingsParams(
                expectedRevision: revision,
                patch: SettingsPatch(
                    defaultStartup: nil, updateChannel: nil, telemetry: nil,
                    auditRetentionDays: days)))
    }

    private func sentLocations(_ daemon: SettingsStoreTests.Daemon) -> [Call] {
        daemon.calls.filter {
            if case .storageSetLocation = $0 { return true } else { return false }
        }
    }

    @Test func aCancelledPanelAsksNothingAndSendsNothing() async {
        let (model, daemon) = await rig(picking: nil)
        await model.change(.data, currentPath: "/x")
        #expect(model.confirmation == nil)
        #expect(sentLocations(daemon).isEmpty)
    }

    @Test func aChosenFolderAsksForConfirmationBeforeAnythingIsSent() async {
        let (model, daemon) = await rig(picking: "/Volumes/Work")
        await model.change(.data, currentPath: "/x")
        let request = model.confirmation
        #expect(request?.title == "Use “/Volumes/Work” for instance data?")
        #expect(
            request?.message
                == "Only instances you create from now on use this folder. Existing data stays where it is."
        )
        #expect(request?.actionTitle == "Use Folder")
        #expect(request?.role == .standard)
        #expect(sentLocations(daemon).isEmpty)
    }

    @Test func confirmingSendsTheChange() async throws {
        let (model, daemon) = await rig(picking: "/Volumes/Work")
        await model.change(.cache, currentPath: "/x")
        let request = try #require(model.confirmation)
        request.perform()
        while sentLocations(daemon).isEmpty { await Task.yield() }
        #expect(
            sentLocations(daemon)
                == [
                    .storageSetLocation(
                        SetLocationParams(
                            kind: .cache, target: .path("/Volumes/Work"), expectedRevision: 3))
                ])
    }

    @Test func useDefaultSendsTheBuiltInTargetWithoutAsking() async {
        let (model, daemon) = await rig(picking: nil)
        await model.useDefault(.logs)
        #expect(model.confirmation == nil)
        #expect(
            sentLocations(daemon)
                == [
                    .storageSetLocation(
                        SetLocationParams(kind: .logs, target: .builtIn, expectedRevision: 3))
                ])
    }

    @Test func revealShowsTheFolderInFinder() async {
        let revealed = Mutex<[URL]>([])
        let daemon = SettingsStoreTests.Daemon()
        let model = StorageSettingsModel(
            store: SettingsStoreTests.store(daemon), picker: FixedPicker(path: nil),
            reveal: { url in revealed.withLock { $0.append(url) } })
        model.revealInFinder("/Users/me/Stackly")
        #expect(revealed.withLock { $0 } == [URL(fileURLWithPath: "/Users/me/Stackly")])
    }

    // MARK: - Keep activity for (spec 0035, AC-20)

    @Test func aLongerActivityRetentionGoesOutAtOnceWithTheRevision() async {
        let (model, store, daemon) = await auditRig(days: 30)
        #expect(store.auditRetentionDays == 30)
        await model.setAuditRetention(90)
        #expect(model.confirmation == nil)
        #expect(sentUpdates(daemon) == [Self.auditPatch(90)])
        #expect(store.auditRetentionDays == 90)
        #expect(store.settings?.auditRetention?.maxRows == 100_000)
    }

    @Test func aShorterActivityRetentionAsksFirstWithCancelAsTheDefault() async throws {
        let (model, store, daemon) = await auditRig(days: 30)
        await model.setAuditRetention(7)
        let request = try #require(model.confirmation)
        #expect(request.title == "Remove activity older than 7 days?")
        #expect(request.message == "Older entries are removed now and can’t be brought back.")
        #expect(request.actionTitle == "Remove")
        #expect(request.role == .destructive)
        #expect(sentUpdates(daemon).isEmpty)
        #expect(store.auditRetentionDays == 30)

        request.perform()
        await settle("the change") { store.auditRetentionDays == 7 && store.saving == nil }
        #expect(sentUpdates(daemon) == [Self.auditPatch(7)])
    }

    @Test func theSameActivityRetentionSendsNothing() async {
        let (model, _, daemon) = await auditRig(days: 30)
        await model.setAuditRetention(30)
        #expect(model.confirmation == nil)
        #expect(sentUpdates(daemon).isEmpty)
    }

    @Test func aDaemonWithoutActivityRetentionGetsNoChange() async {
        let (model, store, daemon) = await auditRig(days: 30)
        daemon.replace(FixtureSettings.view(revision: 9))
        await store.refresh()
        #expect(store.auditRetentionDays == nil)
        await model.setAuditRetention(7)
        #expect(model.confirmation == nil)
        #expect(sentUpdates(daemon).isEmpty)
    }

    @Test func aStaleRevisionOnTheActivityRetentionShowsChangedElsewhere() async {
        let (model, store, daemon) = await auditRig(days: 30)
        daemon.fail(next: SettingsStoreTests.mismatch())
        await model.setAuditRetention(90)
        #expect(store.notices[.auditRetention] == "Changed elsewhere. Try again.")
        #expect(store.auditRetentionDays == 30)
    }
}
