import StacklyClient
import SwiftUI

/// The Instances tab: defaults for the instances you create (spec 0032 AC-18) and the
/// Notifications row (AC-22), which belongs to the app and works while stacklyd is offline.
struct InstancesSettingsTab: View {
    let model: DaemonModel
    let notifications: NotificationModel
    let preferences: AppPreferences

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
                if SettingsAvailability.isShown(.notifications) {
                    NotificationsSettingsRow(notifications: notifications, preferences: preferences)
                }
            }
        }
        .formStyle(.grouped)
        .task { await notifications.refreshPermission() }
    }
}

/// The Sources tab: where the catalog came from, who publishes each provider (spec 0032,
/// *Baris Settings*), and the imported providers with Import Provider… (spec 0037, AC-16).
/// Import runs the same ``ProviderImportModel`` as the Catalog page, with its sheet in the
/// Settings window. While stacklyd is offline the tab shows the last known rows and the
/// button is off (spec 0032 AC-24).
struct SourcesSettingsTab: View {
    let model: DaemonModel
    /// Opens the main window on the Catalog with a provider selected, or `nil` where no
    /// main window can open (a snapshot): then Show in Catalog does not show.
    var showInCatalog: (@MainActor (String) -> Void)?
    /// Asks the person for a folder; the open panel by default.
    var picker: any FolderPicking = OpenPanelFolderPicker()

    @State private var importModel: ProviderImportModel?
    @Environment(\.locale) private var locale
    @Environment(\.calendar) private var calendar
    @Environment(\.timeZone) private var timeZone

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
                        if catalog.revocations.isEmpty == false {
                            Text(ProviderTrustCopy.revokedRevisions(catalog.revocations.count))
                                .font(.callout)
                                .foregroundStyle(StacklyColor.textSecondary)
                        }
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
                    // Imported providers have their own group below.
                    ForEach(providers.filter { $0.trust != .local }, id: \.provider) { provider in
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
            if SettingsAvailability.isShown(.importProviders) {
                importedSection
            }
        }
        .formStyle(.grouped)
        .sheet(isPresented: importSheetShown) {
            if let importModel {
                ImportProviderSheet(model: importModel)
            }
        }
        .task(id: localKeys) {
            for provider in catalog.localProviders ?? [] {
                await catalog.detail.load(provider.provider, revision: provider.revision)
            }
        }
    }

    /// The group of imported providers and Import Provider….
    @ViewBuilder private var importedSection: some View {
        Section(ProviderTrustCopy.settingsGroup) {
            let local = catalog.localProviders ?? []
            if local.isEmpty {
                Text(ProviderTrustCopy.settingsEmpty)
                    .foregroundStyle(StacklyColor.textSecondary)
            }
            ForEach(local, id: \.provider) { provider in
                LabeledContent {
                    if let showInCatalog {
                        Button(ProviderTrustCopy.showInCatalog) {
                            showInCatalog(provider.provider)
                        }
                    }
                } label: {
                    VStack(alignment: .leading, spacing: Spacing.s2) {
                        Text(provider.displayName)
                        Text(line(of: provider))
                            .font(.callout)
                            .foregroundStyle(StacklyColor.textSecondary)
                    }
                }
            }
            HStack {
                Spacer()
                Button(ProviderTrustCopy.importButton) { beginImport() }
                    .disabled(reason: mode == .live ? nil : ProviderTrustCopy.offlineHelp)
            }
        }
    }

    /// The IDs and revisions of the imported providers, so their details load again when
    /// the list changes.
    /// The task key of the detail loads: each imported entry, and the detail epoch, since a
    /// new snapshot drops the cached details and the dates with them.
    private var localKeys: [String] {
        (catalog.localProviders ?? []).map { "\($0.provider)@\($0.revision)" }
            + ["epoch \(catalog.detail.epoch)"]
    }

    /// "{id} · {approval} · imported {date}"; the date shows once the detail loaded.
    private func line(of provider: ProviderSummary) -> String {
        var importedAt: Date?
        if case .loaded(let detail) = catalog.detail.phase(
            for: provider.provider, revision: provider.revision)
        {
            importedAt = detail.local?.importedAt
        }
        return ProviderTrustCopy.settingsLine(
            provider: provider.provider, approval: provider.approval, importedAt: importedAt,
            style: LocalProviderSection.dateStyle(
                locale: locale, calendar: calendar, timeZone: timeZone))
    }

    private func beginImport() {
        let model = importModel ?? catalog.makeImportModel(picker: picker, imported: { _ in })
        importModel = model
        Task { await model.begin() }
    }

    private var importSheetShown: Binding<Bool> {
        Binding(
            get: { importModel?.isPresented ?? false },
            set: { shown in
                if shown == false { importModel?.cancel() }
            })
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
