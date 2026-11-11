import StacklyClient
import SwiftUI
import Testing

@testable import StacklyUI

/// Each tab of the Settings window in the scenarios of spec 0032 (*Fixture*, AC-26), light and
/// dark: General, Instances with notification permission denied, Sources, Storage with custom
/// and old locations, Updates, the last known values while stacklyd is away, and a change the
/// daemon refused because the settings changed elsewhere.
extension SnapshotTests {
    struct SettingsSnapshotTests {
        private static let size = SnapshotSize.fitting(
            width: SettingsRoot.size.width, narrowWidth: SettingsRoot.size.width)
        private static let variants: [SnapshotVariant] = [.light, .dark]

        /// The runtime of a scenario, once the settings and the locations were read.
        @MainActor
        private static func runtime(_ id: FixtureScenario.ID) async throws -> ShellRuntime {
            let runtime = try await settledRuntime(FixtureScenario(id))
            try await waitUntil {
                runtime.model.settings.settings != nil
                    && runtime.model.settings.locations.isEmpty == false
            }
            await runtime.notifications.refreshPermission()
            return runtime
        }

        /// A tab as the Settings window draws it: its size and its environment.
        @MainActor
        private static func window<Tab: View>(_ tab: Tab, _ runtime: ShellRuntime) -> some View {
            tab
                .environment(runtime.model)
                .environment(runtime.preferences)
                .environment(\.mutationAvailability, runtime.model.mutationAvailability)
                .frame(width: SettingsRoot.size.width, height: SettingsRoot.size.height)
        }

        @MainActor
        private static func snapshot<Tab: View>(
            _ tab: Tab, _ runtime: ShellRuntime, named name: String,
            sourceLocation: SourceLocation = #_sourceLocation
        ) async throws {
            defer { runtime.willTerminate() }
            try await assertSnapshot(
                window(tab, runtime), named: name, size: size, variants: variants,
                sourceLocation: sourceLocation)
        }

        @Test(
            .evidence(
                covers: ["REQ-046", "NFR-012"],
                expected:
                    "The General tab shows Appearance, the Startup group, and the Daemon section"
            ))
        @MainActor
        func general() async throws {
            let runtime = try await Self.runtime(.settingsGeneral)
            try await Self.snapshot(
                GeneralSettingsTab(loginItem: runtime.loginItem), runtime, named: "settings-general"
            )
        }

        @Test(
            .evidence(
                covers: ["REQ-046", "NFR-012"],
                expected:
                    "The Instances tab shows Autostart new instances and the Notifications row with the denied text"
            ))
        @MainActor
        func instancesDenied() async throws {
            let runtime = try await Self.runtime(.settingsInstancesDenied)
            try await Self.snapshot(
                InstancesSettingsTab(
                    model: runtime.model, notifications: runtime.notifications,
                    preferences: runtime.preferences),
                runtime, named: "settings-instances-denied")
        }

        @Test(
            .evidence(
                covers: ["REQ-046", "NFR-012"],
                expected:
                    "The Sources tab shows the catalog source and each provider with its publisher"
            ))
        @MainActor
        func sources() async throws {
            let runtime = try await Self.runtime(.settingsSources)
            try await Self.snapshot(
                SourcesSettingsTab(model: runtime.model), runtime, named: "settings-sources")
        }

        @Test(
            .evidence(
                covers: ["REQ-033", "NFR-012"],
                expected:
                    "The Sources tab lists the imported providers with their approval and import date, the catalog revocations, and Import Provider…"
            ))
        @MainActor
        func sourcesImported() async throws {
            let runtime = try await Self.runtime(.settingsSourcesImported)
            let catalog = runtime.model.catalog
            try await waitUntil { catalog.localProviders?.isEmpty == false }
            for provider in catalog.localProviders ?? [] {
                await catalog.detail.load(provider.provider, revision: provider.revision)
            }
            try await Self.snapshot(
                SourcesSettingsTab(model: runtime.model, showInCatalog: { _ in }), runtime,
                named: "settings-sources-imported")
        }

        @Test(
            .evidence(
                covers: ["REQ-046", "NFR-012"],
                expected:
                    "The Storage tab shows custom folders, the old ones still used, the unreachable one, and the log retention"
            ))
        @MainActor
        func storageCustom() async throws {
            let runtime = try await Self.runtime(.settingsStorageCustom)
            try await Self.snapshot(
                StorageSettingsTab(model: runtime.model), runtime, named: "settings-storage-custom")
        }

        @Test(
            .evidence(
                covers: ["REQ-045", "NFR-012"],
                expected:
                    "Below Logs the Storage tab has the Activity group: Keep activity for 30 days and the entry limit (spec 0035 AC-20)"
            ))
        @MainActor
        func storageActivity() async throws {
            let runtime = try await Self.runtime(.settingsGeneral)
            defer { runtime.willTerminate() }
            // Taller than the window, so the whole form shows down to the Activity group.
            try await assertSnapshot(
                StorageSettingsTab(model: runtime.model)
                    .environment(runtime.model)
                    .environment(runtime.preferences)
                    .environment(\.mutationAvailability, runtime.model.mutationAvailability)
                    .frame(width: SettingsRoot.size.width, height: 720),
                named: "settings-storage-activity", size: Self.size, variants: Self.variants)
        }

        @Test(
            .evidence(
                covers: ["REQ-046", "NFR-012"],
                expected:
                    "The Updates tab shows the beta channel and statistics on, with the honest notes"
            ))
        @MainActor
        func updates() async throws {
            let runtime = try await Self.runtime(.settingsUpdates)
            try await Self.snapshot(
                UpdatesSettingsTab(model: runtime.model), runtime, named: "settings-updates")
        }

        @Test(
            .evidence(
                covers: ["REQ-046", "NFR-012"],
                expected:
                    "While stacklyd is unreachable the Storage tab shows the last known values, off, with the banner"
            ))
        @MainActor
        func offline() async throws {
            let runtime = try await Self.runtime(.settingsOffline)
            try await Self.snapshot(
                StorageSettingsTab(model: runtime.model), runtime, named: "settings-offline")
        }

        @Test(
            .evidence(
                covers: ["REQ-046", "NFR-012"],
                expected:
                    "A change refused for a stale revision shows the daemon's value and Changed elsewhere. Try again."
            ))
        @MainActor
        func conflict() async throws {
            let runtime = try await Self.runtime(.settingsConflict)
            await runtime.model.settings.apply(.telemetry(false))
            try await Self.snapshot(
                UpdatesSettingsTab(model: runtime.model), runtime, named: "settings-conflict")
        }
    }
}
