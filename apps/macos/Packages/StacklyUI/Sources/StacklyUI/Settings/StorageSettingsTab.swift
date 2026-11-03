import StacklyClient
import SwiftUI

/// The Storage tab (spec 0032 AC-19, AC-21, AC-24): where the data, logs, and downloads of
/// instances you create from now on go, with Change…, Use Default, and Reveal in Finder, and the
/// read only line about how much of each log is kept. Nothing here moves or deletes data that
/// is already on disk.
struct StorageSettingsTab: View {
    let model: DaemonModel
    @State private var actions: StorageSettingsModel

    init(model: DaemonModel, actions: StorageSettingsModel? = nil) {
        self.model = model
        _actions = State(initialValue: actions ?? StorageSettingsModel(store: model.settings))
    }

    private var store: SettingsStore { model.settings }

    private var mode: SettingsRowMode {
        SettingsCopy.mode(isConnected: model.presentation.hasConnection, hasLoaded: store.hasLoaded)
    }

    var body: some View {
        Form {
            if mode != .live {
                Section { SettingsOfflineBanner(hasLoaded: store.hasLoaded) }
            }
            Section("Locations") {
                if mode == .unavailable {
                    Text(SettingsCopy.notAvailable)
                        .foregroundStyle(StacklyColor.textSecondary)
                } else if let settings = store.settings {
                    ForEach(rows(settings)) { row in
                        LocationRow(row: row, mode: mode, store: store, actions: actions)
                    }
                }
            }
            if SettingsAvailability.isShown(.logRetention), let settings = store.settings {
                Section("Logs") {
                    LabeledContent {
                        EmptyView()
                    } label: {
                        VStack(alignment: .leading, spacing: Spacing.s2) {
                            Text("Instance logs")
                            Text(LocationRowContent.retentionText(settings.logRetention))
                                .font(.callout)
                                .foregroundStyle(StacklyColor.textSecondary)
                                .fixedSize(horizontal: false, vertical: true)
                        }
                    }
                }
            }
        }
        .formStyle(.grouped)
        .stacklyConfirmation(item: $actions.confirmation)
    }

    private func rows(_ settings: DaemonSettingsView) -> [LocationRowContent] {
        let shown: [(SettingsRow, LocationKind)] = [
            (.dataLocation, .data), (.logsLocation, .logs), (.cacheLocation, .cache),
        ]
        return shown.compactMap { row, kind in
            guard SettingsAvailability.isShown(row) else { return nil }
            return LocationRowContent.make(
                kind: kind, settings: settings, locations: store.locations,
                isConnected: model.presentation.hasConnection)
        }
    }
}

extension LocationRowContent: Identifiable {
    var id: LocationKind.RawValue { kind.rawValue }
}

/// One location row: the folder, what it is used for, its buttons, and the older locations.
private struct LocationRow: View {
    let row: LocationRowContent
    let mode: SettingsRowMode
    let store: SettingsStore
    let actions: StorageSettingsModel

    private var isSaving: Bool { store.savingLocation == row.kind }
    private var isBusy: Bool { store.savingLocation != nil }

    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.s8) {
            LabeledContent {
                HStack(spacing: Spacing.s8) {
                    if isSaving {
                        ProgressView().controlSize(.small)
                    }
                    if mode == .lastKnown {
                        Text(SettingsCopy.lastKnown)
                            .font(.callout)
                            .foregroundStyle(StacklyColor.textSecondary)
                    }
                    buttons
                }
            } label: {
                VStack(alignment: .leading, spacing: Spacing.s2) {
                    Text(row.title)
                    Text(row.path)
                        .font(.callout)
                        .foregroundStyle(StacklyColor.textSecondary)
                        .textSelection(.enabled)
                        .lineLimit(2)
                        .truncationMode(.middle)
                    Text(row.usage)
                        .font(.callout)
                        .foregroundStyle(StacklyColor.textSecondary)
                    if let status = row.status {
                        Text(status)
                            .font(.callout)
                            .foregroundStyle(StacklyColor.textSecondary)
                    }
                    if let notice = store.locationNotices[row.kind] {
                        Text(notice)
                            .font(.callout)
                            .foregroundStyle(StacklyColor.textSecondary)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                }
            }
            ForEach(row.older) { older in
                VStack(alignment: .leading, spacing: Spacing.s2) {
                    Text(older.path)
                        .font(.callout)
                        .textSelection(.enabled)
                        .lineLimit(2)
                        .truncationMode(.middle)
                    Text(older.usage)
                        .font(.callout)
                        .foregroundStyle(StacklyColor.textSecondary)
                    if let status = older.status {
                        Text(status)
                            .font(.callout)
                            .foregroundStyle(StacklyColor.textSecondary)
                    }
                }
            }
        }
    }

    @ViewBuilder private var buttons: some View {
        let live = mode == .live
        Button("Reveal in Finder") { actions.revealInFinder(row.path) }
            .disabled(live == false && mode == .unavailable)
        if row.showsUseDefault {
            Button("Use Default") { Task { await actions.useDefault(row.kind) } }
                .disabled(live == false || isBusy)
        }
        Button("Change…") { Task { await actions.change(row.kind, currentPath: row.path) } }
            .disabled(live == false || isBusy)
    }
}
