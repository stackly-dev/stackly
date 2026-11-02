import StacklyClient
import SwiftUI

/// The Configuration tab of the detail page (spec 0029, *GUI*): the settings form built from
/// the schema of the pinned package, the files Stackly writes, the pending changes, and the
/// history of revisions, with the banners for a file changed outside Stackly and for a
/// revision that never reached ready.
struct ConfigurationTab: View {
    @Environment(\.shellNavigator) private var navigator
    @Environment(\.actionHost) private var host
    @Environment(\.mutationAvailability) private var gate
    @Environment(\.locale) private var locale
    @Environment(\.loadingDelay) private var loadingDelay
    @State private var showsDiff = false

    let model: ConfigurationModel
    let summary: InstanceSummary
    let context: InstanceActionContext

    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.sectionGap) {
            switch model.phase {
            case .idle, .loading:
                DelayedProgress(delay: loadingDelay ?? DelayedVisibility.defaultDelay)
            case .failed(let body):
                StatePlaceholder(
                    FailureContent(
                        title: "Couldn’t Load Configuration", message: body.message,
                        code: body.code),
                    retry: { Task { await model.reload() } },
                    openDiagnostics: { navigator?.show(.diagnostics) })
            case .loaded:
                if let view = model.view {
                    loaded(view)
                }
            }
        }
        .task { await model.open() }
        .onChange(of: summary.revision) { _, _ in Task { await model.refresh() } }
        .sheet(isPresented: $showsDiff) {
            if let drift = model.view?.drift {
                DriftDiffSheet(
                    drift: drift,
                    keep: model.keepRevisionAction(name: summary.name, context: context))
            }
        }
    }

    // MARK: - Loaded

    @ViewBuilder private func loaded(_ view: InstanceConfigView) -> some View {
        header(view)
        applyReason(view)
        banners(view)
        switch model.mode {
        case .form:
            if let schema = model.schema {
                ConfigurationForm(schema: schema, endpoints: view.endpoints)
            }
            pendingChanges
            history(view)
        case .effectiveFile:
            EffectiveFilesView(files: model.shownFiles, note: view.effectiveFilesNote)
        }
    }

    private func header(_ view: InstanceConfigView) -> some View {
        HStack(spacing: Spacing.s12) {
            Picker("View", selection: Binding(get: { model.mode }, set: { model.mode = $0 })) {
                ForEach(ConfigurationModel.Mode.allCases) { Text($0.title).tag($0) }
            }
            .pickerStyle(.segmented)
            .labelsHidden()
            .fixedSize()
            Spacer(minLength: Spacing.s12)
            Text("Revision \(view.revision.number)")
                .font(.headline)
            ToneBadge(
                StatusAppearance.tag(model.impactTag, tone(model.impact)))
            ActionButton(model.revertAction, showsReason: false)
            ActionButton(
                model.applyAction(name: summary.name, context: context), prominence: .primary,
                showsReason: false)
        }
    }

    /// Why Apply is off, under the header, while a change waits or a banner blocks it.
    @ViewBuilder private func applyReason(_ view: InstanceConfigView) -> some View {
        if model.busy == nil, model.hasPending || view.drift != nil || model.conflict != nil,
            let reason = model.applyDisabledReason
        {
            DisabledReasonText(reason)
                .frame(maxWidth: .infinity, alignment: .trailing)
        }
    }

    private func tone(_ impact: ConfigImpact) -> Tone {
        switch impact {
        case .none, .unrecognized: .neutral
        case .nextStart: .informative
        case .reload, .restart: .caution
        }
    }

    // MARK: - Banners

    @ViewBuilder private func banners(_ view: InstanceConfigView) -> some View {
        if let active = model.conflict {
            StateBanner(
                tone: .caution,
                title: "This configuration changed. Reload to see revision \(active).",
                message: "Your pending changes stay until you reload.",
                actions: [
                    StateAction(
                        title: "Reload", mutates: false,
                        perform: { Task { await model.reload() } })
                ])
        }
        if let drift = view.drift {
            StateBanner(
                tone: .caution, title: ConfigurationCopy.driftTitle(drift),
                message: ConfigurationCopy.driftMessage(
                    drift, name: summary.name, locale: locale),
                actions: driftActions)
        }
        if let target = model.rollBackTarget,
            let action = model.rollBackAction(
                name: summary.name, context: context)
        {
            StateBanner(
                tone: .caution,
                title: ConfigurationCopy.rollBackTitle(active: view.revision.number),
                message: ConfigurationCopy.rollBackMessage(lastKnownGood: target),
                actions: [stateAction(action)])
        }
        if let error = model.validationError {
            StateBanner(
                tone: .critical, title: "Couldn’t check the changes", message: error.message,
                code: error.code)
        }
    }

    private var driftActions: [StateAction] {
        var actions: [StateAction] = []
        if let keep = model.keepRevisionAction(name: summary.name, context: context) {
            actions.append(stateAction(keep))
        }
        actions.append(
            StateAction(title: "Show Diff", mutates: false, perform: { showsDiff = true }))
        return actions
    }

    /// A banner button for an action, run through the window's action host so its
    /// confirmation shows.
    private func stateAction(_ action: StacklyAction) -> StateAction {
        StateAction(
            title: action.title, mutates: action.mutates, isBusy: action.isBusy,
            perform: { action.run(host: host, gate: gate) })
    }

    // MARK: - Pending changes

    private var pendingChanges: some View {
        CardSection("Pending Changes") {
            let lines = pendingLines
            if lines.isEmpty {
                Text(ConfigurationCopy.noPendingChanges)
                    .foregroundStyle(StacklyColor.textSecondary)
            } else {
                DiffView(lines, emptyText: ConfigurationCopy.noPendingChanges)
            }
        }
    }

    /// The `key = old` and `key = new` lines of the verdict, then the ports that move.
    private var pendingLines: [DiffLine] {
        guard model.hasPending, model.isValidating == false, let validation = model.validation,
            validation.valid
        else { return [] }
        var lines: [DiffLine] = []
        func add(_ kind: DiffKind, _ text: String) {
            lines.append(DiffLine(id: lines.count, kind: kind, text: text))
        }
        for change in validation.changes {
            if let old = change.old {
                add(.removed, ConfigurationCopy.settingLine(key: change.key, value: old))
            }
            if let new = change.new {
                add(.added, ConfigurationCopy.settingLine(key: change.key, value: new))
            } else {
                add(.added, ConfigurationCopy.settingLine(key: change.key, value: nil))
            }
        }
        for change in validation.endpointChanges {
            add(
                .removed,
                ConfigurationCopy.settingLine(
                    key: "port \(change.name)", value: ConfigurationCopy.portText(change.old)))
            add(
                .added,
                ConfigurationCopy.settingLine(
                    key: "port \(change.name)", value: ConfigurationCopy.portText(change.new)))
        }
        return lines
    }

    // MARK: - History

    private func history(_ view: InstanceConfigView) -> some View {
        CardSection("History") {
            ForEach(view.history, id: \.number) { revision in
                HistoryRow(revision: revision, restore: { model.restore(revision) })
            }
        }
    }
}

extension StatusAppearance {
    /// A badge of the tab: `label` in `tone`, with the symbol of the tone.
    static func tag(_ label: String, _ tone: Tone) -> StatusAppearance {
        StatusAppearance(label: label, tone: tone, symbol: tone.symbol)
    }
}

/// One row of History: the number, how it came to be and when, its badges, and Restore.
private struct HistoryRow: View {
    @Environment(\.locale) private var locale

    let revision: ConfigRevisionSummary
    let restore: () -> Void

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: Spacing.s12) {
            VStack(alignment: .leading, spacing: Spacing.s2) {
                HStack(spacing: Spacing.s8) {
                    Text("Revision \(revision.number)")
                        .fontWeight(.medium)
                    if revision.isActive {
                        ToneBadge(StatusAppearance.tag("Active", .informative))
                    }
                    if revision.isLastKnownGood {
                        ToneBadge(
                            StatusAppearance.tag("Last known good", .positive))
                    }
                }
                Text("\(ConfigurationCopy.origin(revision)) · \(time)")
                    .font(.callout)
                    .foregroundStyle(StacklyColor.textSecondary)
            }
            Spacer(minLength: Spacing.s12)
            if revision.isActive == false {
                Button("Restore", action: restore)
            }
        }
    }

    private var time: String {
        revision.createdAt.formatted(
            Date.FormatStyle(date: .abbreviated, time: .shortened).locale(locale))
    }
}

/// The settings form of an existing instance: the sections of the schema form, with the
/// ports as locked rows under them.
struct ConfigurationForm: View {
    let schema: SchemaFormModel
    let endpoints: [ConfigEndpoint]

    var body: some View {
        Form {
            SchemaFormSections(schema: schema, drafts: .constant([:]))
            let ipEndpoints = endpoints.filter { $0.changeable }
            if ipEndpoints.isEmpty == false {
                Section("Ports") {
                    ForEach(ipEndpoints, id: \.name) { endpoint in
                        LabeledContent(endpoint.name) {
                            VStack(alignment: .trailing, spacing: Spacing.s2) {
                                Label(portValue(endpoint), systemImage: "lock.fill")
                                    .font(.body.monospaced())
                                Text(ConfigurationCopy.portRow)
                                    .font(.callout)
                                    .foregroundStyle(StacklyColor.textSecondary)
                            }
                        }
                    }
                }
            }
        }
        .formStyle(.grouped)
        .scrollContentBackground(.hidden)
        .scrollDisabled(true)
    }

    private func portValue(_ endpoint: ConfigEndpoint) -> String {
        if let port = endpoint.boundPort { return String(port) }
        return endpoint.request.map(ConfigurationCopy.portText) ?? "Automatic"
    }
}

/// The Effective File view: one read only block per file, the changed lines of a pending
/// change marked, with the note for an Automatic port above it.
struct EffectiveFilesView: View {
    let files: [EffectiveFile]
    let note: String?

    var body: some View {
        if files.isEmpty {
            Text(note ?? "This provider writes no configuration files you can read.")
                .foregroundStyle(StacklyColor.textSecondary)
        }
        ForEach(files, id: \.path) { file in
            CardSection(file.path) {
                if let portNote = file.portNote {
                    Text(portNote)
                        .font(.callout)
                        .foregroundStyle(StacklyColor.textSecondary)
                }
                DiffView(Self.lines(of: file), emptyText: "This file is empty.")
            }
        }
    }

    /// The lines of a file, those in `changedLines` marked as added.
    static func lines(of file: EffectiveFile) -> [DiffLine] {
        let changed = Set(file.changedLines)
        let texts = file.content.split(separator: "\n", omittingEmptySubsequences: false)
        let trimmed = texts.last == "" ? texts.dropLast() : texts[...]
        return trimmed.enumerated().map { index, text in
            DiffLine(
                id: index, kind: changed.contains(UInt32(index + 1)) ? .added : .unchanged,
                text: String(text))
        }
    }
}

/// The diff of every file changed outside Stackly, with Keep Revision when the tab can run it.
struct DriftDiffSheet: View {
    @Environment(\.dismiss) private var dismiss

    let drift: ConfigDrift
    let keep: StacklyAction?

    var body: some View {
        StacklySheet(
            title: "Changes Outside Stackly",
            subtitle: "What differs between the files on disk and the last revision Stackly wrote.",
            primary: keep
                ?? StacklyAction(
                    id: "drift.close", title: "Close", mutates: false, perform: { dismiss() }),
            cancel: keep.map { _ in
                StacklyAction(
                    id: "drift.cancel", title: "Close", mutates: false, perform: { dismiss() })
            },
            minWidth: 560
        ) {
            ForEach(drift.files, id: \.path) { file in
                VStack(alignment: .leading, spacing: Spacing.s6) {
                    HStack(spacing: Spacing.s8) {
                        Text(file.path).fontWeight(.medium)
                        ToneBadge(
                            StatusAppearance.tag(
                                file.kind == .missing ? "Missing" : "Modified", .caution))
                    }
                    DiffView(Self.lines(of: file), emptyText: "No differences.")
                }
                .frame(maxWidth: .infinity, alignment: .leading)
            }
        }
    }

    /// The lines of a drifted file as the shared diff view draws them.
    static func lines(of file: DriftFile) -> [DiffLine] {
        file.diff.enumerated().map { index, line in
            let kind: DiffKind =
                switch line.kind {
                case .added: .added
                case .removed: .removed
                case .context, .unrecognized: .unchanged
                }
            return DiffLine(id: index, kind: kind, text: line.text)
        }
    }
}
