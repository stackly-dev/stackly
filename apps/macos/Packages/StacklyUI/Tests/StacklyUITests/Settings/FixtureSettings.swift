import Foundation
import StacklyClient

@testable import StacklyUI

/// Settings, locations, and events for the Settings tests, built from the generated DTOs.
nonisolated enum FixtureSettings {
    static let updatedAt = Date(timeIntervalSince1970: 1_792_300_000)

    static func location(
        _ kind: LocationKind, number: UInt8, path: String? = nil, builtIn: Bool = true,
        isDefault: Bool = true, instanceCount: UInt64 = 0, reachable: Bool = true
    ) -> StorageLocationView {
        StorageLocationView(
            id: FixtureID.uuid(kind: 0x51, number: number), kind: kind,
            path: path ?? "/Users/test/Library/Application Support/Stackly/\(kind.rawValue)",
            builtIn: builtIn, isDefault: isDefault, instanceCount: instanceCount,
            reachable: reachable)
    }

    static let dataLocation = location(.data, number: 1)
    static let logsLocation = location(.logs, number: 2)
    static let cacheLocation = location(.cache, number: 3)

    static var locations: [StorageLocationView] { [dataLocation, logsLocation, cacheLocation] }

    static func view(
        revision: UInt64 = 3, startup: StartupPolicyKind = .manual,
        channel: UpdateChannel = .stable, telemetry: Bool = false,
        data: StorageLocationView = dataLocation, logs: StorageLocationView = logsLocation,
        cache: StorageLocationView = cacheLocation, auditDays: UInt32? = nil
    ) -> DaemonSettingsView {
        DaemonSettingsView(
            revision: revision, defaultStartup: startup, updateChannel: channel,
            telemetry: telemetry, telemetryCopyVersion: 1,
            locations: DefaultLocations(data: data, logs: logs, cache: cache),
            logRetention: LogRetention(segmentsPerStream: 8, segmentMaxBytes: 1_048_576),
            auditRetention: auditDays.map { AuditRetention(days: $0, maxRows: 100_000) },
            updatedAt: updatedAt)
    }

    /// What the daemon does with a patch: sets what it names and raises the revision.
    static func apply(_ patch: SettingsPatch, to settings: DaemonSettingsView)
        -> DaemonSettingsView
    {
        DaemonSettingsView(
            revision: settings.revision + 1,
            defaultStartup: patch.defaultStartup ?? settings.defaultStartup,
            updateChannel: patch.updateChannel ?? settings.updateChannel,
            telemetry: patch.telemetry ?? settings.telemetry,
            telemetryCopyVersion: settings.telemetryCopyVersion, locations: settings.locations,
            logRetention: settings.logRetention,
            auditRetention: patch.auditRetentionDays.map {
                AuditRetention(days: $0, maxRows: settings.auditRetention?.maxRows ?? 100_000)
            } ?? settings.auditRetention,
            updatedAt: settings.updatedAt)
    }

    static func event(revision: UInt64) -> Event {
        Event(
            subscriptionId: 1, sequence: revision, at: FixtureClock.now, resource: nil,
            revision: nil, kind: .settingsChanged(SettingsChanged(revision: revision)))
    }

    /// The settings after the default of `location.kind` moved to `location`.
    static func moving(_ settings: DaemonSettingsView, to location: StorageLocationView)
        -> DaemonSettingsView
    {
        let old = settings.locations
        let locations = DefaultLocations(
            data: location.kind == .data ? location : old.data,
            logs: location.kind == .logs ? location : old.logs,
            cache: location.kind == .cache ? location : old.cache)
        return DaemonSettingsView(
            revision: settings.revision + 1, defaultStartup: settings.defaultStartup,
            updateChannel: settings.updateChannel, telemetry: settings.telemetry,
            telemetryCopyVersion: settings.telemetryCopyVersion, locations: locations,
            logRetention: settings.logRetention, auditRetention: settings.auditRetention,
            updatedAt: settings.updatedAt)
    }
}
