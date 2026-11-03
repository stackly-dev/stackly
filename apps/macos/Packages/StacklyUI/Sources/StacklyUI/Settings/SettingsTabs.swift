import StacklyClient
import SwiftUI

/// The Instances tab: defaults for the instances you create (spec 0032 AC-18). The
/// Notifications row comes with the notification model and shows once `SettingsAvailability`
/// says so.
struct InstancesSettingsTab: View {
    let model: DaemonModel

    private var store: SettingsStore { model.settings }

    private var mode: SettingsRowMode {
        SettingsCopy.mode(isConnected: model.presentation.hasConnection, hasLoaded: store.hasLoaded)
    }

    var body: some View {
        Form {
            if mode != .live {
                Section { SettingsOfflineBanner(hasLoaded: store.hasLoaded) }
            }
            Section("Defaults") {
                if SettingsAvailability.isShown(.autostartNewInstances) {
                    DaemonSettingRow(
                        title: "Autostart new instances", note: SettingsCopy.autostartNote,
                        mode: mode, isSaving: store.saving?.field == .defaultStartup,
                        notice: store.notices[.defaultStartup]
                    ) {
                        Toggle(
                            "Autostart new instances",
                            isOn: Binding(
                                get: { store.defaultStartup == .atLogin },
                                set: { on in
                                    Task { await store.apply(.startup(on ? .atLogin : .manual)) }
                                }))
                    }
                    .disabled(store.saving != nil && store.saving?.field != .defaultStartup)
                }
            }
        }
        .formStyle(.grouped)
    }
}

/// The Sources tab: where the catalog came from and who publishes each provider (spec 0032,
/// *Baris Settings*). It only reads; the catalog has no setting yet.
struct SourcesSettingsTab: View {
    let model: DaemonModel

    private var catalog: CatalogStore { model.catalog }

    private var mode: SettingsRowMode {
        SettingsCopy.mode(
            isConnected: model.presentation.hasConnection, hasLoaded: catalog.info != nil)
    }

    var body: some View {
        Form {
            if mode != .live {
                Section { SettingsOfflineBanner(hasLoaded: catalog.info != nil) }
            }
            if SettingsAvailability.isShown(.catalogSource) {
                Section("Catalog") {
                    if let info = catalog.info {
                        LabeledContent("Source", value: SettingsCopy.sourceText(info.source))
                        LabeledContent("Trust", value: CatalogCopy.trustText(info.trust))
                        LabeledContent("Revision", value: String(info.revision))
                        LabeledContent(
                            "Published",
                            value: info.publishedAt.formatted(date: .abbreviated, time: .omitted))
                        if mode == .lastKnown {
                            Text(SettingsCopy.lastKnown)
                                .font(.callout)
                                .foregroundStyle(StacklyColor.textSecondary)
                        }
                    } else {
                        Text(SettingsCopy.notAvailable)
                            .foregroundStyle(StacklyColor.textSecondary)
                    }
                }
            }
            if SettingsAvailability.isShown(.providers), let providers = catalog.providers {
                Section("Providers") {
                    ForEach(providers, id: \.provider) { provider in
                        LabeledContent {
                            Text(provider.publisher ?? "Unknown publisher")
                        } label: {
                            VStack(alignment: .leading, spacing: Spacing.s2) {
                                Text(provider.displayName)
                                Text(CatalogCopy.trustText(provider.trust))
                                    .font(.callout)
                                    .foregroundStyle(StacklyColor.textSecondary)
                            }
                        }
                    }
                }
            }
        }
        .formStyle(.grouped)
    }
}

/// The Updates tab: the update channel and the usage statistics choice (spec 0032 AC-23).
/// Both are saved in the daemon; neither changes what Stackly does today, and the notes say so.
struct UpdatesSettingsTab: View {
    let model: DaemonModel

    private var store: SettingsStore { model.settings }

    private var mode: SettingsRowMode {
        SettingsCopy.mode(isConnected: model.presentation.hasConnection, hasLoaded: store.hasLoaded)
    }

    var body: some View {
        Form {
            if mode != .live {
                Section { SettingsOfflineBanner(hasLoaded: store.hasLoaded) }
            }
            if SettingsAvailability.isShown(.updateChannel) {
                Section("Updates") {
                    DaemonSettingRow(
                        title: "Channel", note: SettingsCopy.channelNote, mode: mode,
                        isSaving: store.saving?.field == .updateChannel,
                        notice: store.notices[.updateChannel]
                    ) {
                        Picker(
                            "Channel",
                            selection: Binding(
                                get: {
                                    store.updateChannel == .beta ? UpdateChannel.beta : .stable
                                },
                                set: { value in Task { await store.apply(.channel(value)) } })
                        ) {
                            Text("Stable").tag(UpdateChannel.stable)
                            Text("Beta").tag(UpdateChannel.beta)
                        }
                        .pickerStyle(.segmented)
                        .fixedSize()
                    }
                    .disabled(store.saving != nil && store.saving?.field != .updateChannel)
                }
            }
            if SettingsAvailability.isShown(.telemetry) {
                Section("Privacy") {
                    DaemonSettingRow(
                        title: "Share anonymous usage statistics", note: SettingsCopy.telemetryNote,
                        mode: mode, isSaving: store.saving?.field == .telemetry,
                        notice: store.notices[.telemetry]
                    ) {
                        Toggle(
                            "Share anonymous usage statistics",
                            isOn: Binding(
                                get: { store.telemetry ?? false },
                                set: { on in Task { await store.apply(.telemetry(on)) } }))
                    }
                    .disabled(store.saving != nil && store.saving?.field != .telemetry)
                }
            }
        }
        .formStyle(.grouped)
    }
}
