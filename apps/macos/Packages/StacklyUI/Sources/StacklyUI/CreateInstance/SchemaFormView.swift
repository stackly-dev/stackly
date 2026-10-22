import StacklyClient
import SwiftUI

extension SchemaRow {
    /// What VoiceOver reads for the field: the title, "required" in words, and "set once at
    /// creation" for a field that cannot change later.
    public var spokenLabel: String {
        var label = field.title
        if showsRequired { label += ", required" }
        if isSetOnce { label += ", set once at creation" }
        return label
    }
}

/// The settings sections of the New Instance sheet, drawn from a ``SchemaFormModel`` and
/// nothing else (spec 0020, *Form dari settings schema*). It goes inside the sheet's `Form`.
struct SchemaFormSections: View {
    let schema: SchemaFormModel
    @Binding var drafts: [String: SecretDraft]

    @State private var advancedExpanded = false

    var body: some View {
        if !schema.basicRows.isEmpty {
            Section("Settings") {
                ForEach(schema.basicRows) { row in
                    SchemaFieldRow(schema: schema, row: row, draft: draft(row))
                }
            }
        }
        if !schema.advancedRows.isEmpty {
            Section {
                DisclosureGroup("Advanced", isExpanded: $advancedExpanded) {
                    ForEach(schema.advancedRows) { row in
                        SchemaFieldRow(schema: schema, row: row, draft: draft(row))
                    }
                }
                .accessibilityIdentifier(AccessibilityID.CreateInstance.advanced)
                .onChange(of: schema.advancedHasIssue, initial: true) { _, hasIssue in
                    if hasIssue { advancedExpanded = true }
                }
            }
        }
    }

    /// The draft of a secret row. Setting it also tells the model the source and whether the
    /// typed text fits, never the text.
    private func draft(_ row: SchemaRow) -> Binding<SecretDraft> {
        Binding(
            get: { drafts[row.id] ?? SecretDraft() },
            set: { newValue in
                drafts[row.id] = newValue
                guard case .secret(let limits) = row.control else { return }
                schema.setSecret(
                    row.id,
                    SecretChoice(
                        source: newValue.source, reference: newValue.copyFromReference,
                        isValid: newValue.canSubmit(limits: limits)))
            })
    }
}

/// One settings field: its control, the help under it, and the sentences of its errors.
struct SchemaFieldRow: View {
    let schema: SchemaFormModel
    let row: SchemaRow
    @Binding var draft: SecretDraft

    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.s4) {
            control
            help
            ForEach(errors, id: \.self) { error in
                Label(error, systemImage: Tone.critical.symbol)
                    .font(.callout)
                    .foregroundStyle(Tone.critical.text)
            }
        }
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier(AccessibilityID.CreateInstance.field(row.id))
    }

    private var errors: [String] { schema.errorTexts(for: row.id) }

    private var title: String { row.field.title }

    @ViewBuilder private var control: some View {
        switch row.control {
        case .managed:
            LabeledContent(title) {
                Label("Set by Stackly", systemImage: "lock.fill")
                    .foregroundStyle(StacklyColor.textSecondary)
            }
        case .secret(let limits):
            SecretField(
                title, draft: $draft, limits: limits, allowsGenerate: true,
                copyFromOptions: schema.copyFromOptions, copyFromLabels: schema.copyFromLabels)
        case .text:
            TextField(title, text: text)
                .accessibilityLabel(row.spokenLabel)
                .accessibilityHint(errors.joined(separator: " "))
        case .integer:
            TextField(title, text: text)
                .multilineTextAlignment(.trailing)
                .monospacedDigit()
                .accessibilityLabel(row.spokenLabel)
                .accessibilityHint(errors.joined(separator: " "))
        case .toggle:
            Toggle(title, isOn: toggle)
                .accessibilityLabel(row.spokenLabel)
                .accessibilityHint(errors.joined(separator: " "))
        case .choice(let values):
            Picker(title, selection: text) {
                if text.wrappedValue.isEmpty || !values.contains(text.wrappedValue) {
                    Text("Choose…").tag(text.wrappedValue)
                }
                ForEach(values, id: \.self) { Text($0).tag($0) }
            }
            .pickerStyle(.menu)
            .accessibilityLabel(row.spokenLabel)
            .accessibilityHint(errors.joined(separator: " "))
        case .list:
            SchemaListEditor(schema: schema, row: row)
        case .unrecognized:
            Label(
                "\(title): This setting needs a newer version of Stackly.",
                systemImage: Tone.caution.symbol
            )
            .foregroundStyle(StacklyColor.textSecondary)
        }
    }

    @ViewBuilder private var help: some View {
        let lines = helpLines
        if !lines.isEmpty {
            VStack(alignment: .leading, spacing: Spacing.s2) {
                ForEach(lines, id: \.self) { Text($0) }
            }
            .font(.callout)
            .foregroundStyle(StacklyColor.textSecondary)
        }
        if row.isSetOnce {
            ToneBadge(StatusAppearance(label: "Set once", tone: .neutral, symbol: "lock.fill"))
        }
    }

    /// The sentences under the control: the description, the bounds, "Required", and why a
    /// set once field is final. Words, never only a symbol.
    private var helpLines: [String] {
        var lines: [String] = []
        if let description = row.field.description { lines.append(description) }
        if let bounds = row.boundsHint { lines.append(bounds) }
        if row.showsRequired { lines.append("Required") }
        if row.isSetOnce { lines.append("Can’t be changed after the instance is created.") }
        return lines
    }

    private var text: Binding<String> {
        Binding(get: { schema.text(row.id) }, set: { schema.setText(row.id, $0) })
    }

    private var toggle: Binding<Bool> {
        Binding(get: { schema.isOn(row.id) }, set: { schema.setOn(row.id, $0) })
    }
}

/// The table of a list field: one row per item with a control per item field, Remove on
/// each row, and Add under them, within `minItems` and `maxItems`.
struct SchemaListEditor: View {
    let schema: SchemaFormModel
    let row: SchemaRow

    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.s8) {
            Text(row.field.title)
            ForEach(schema.items(row.id)) { item in
                HStack(alignment: .firstTextBaseline, spacing: Spacing.s8) {
                    ForEach(row.field.items, id: \.key) { itemField in
                        entry(of: item, field: itemField)
                    }
                    Button {
                        schema.deleteItem(from: row.id, id: item.id)
                    } label: {
                        Image(systemName: "minus.circle")
                    }
                    .buttonStyle(.borderless)
                    .accessibilityLabel("Remove \(row.field.title) item")
                    .disabled(!canRemove)
                }
            }
            Button("Add") { schema.addItem(to: row.id) }
                .accessibilityLabel("Add \(row.field.title) item")
                .disabled(!canAdd)
        }
    }

    private var canAdd: Bool {
        guard let maxItems = row.field.maxItems else { return true }
        return schema.items(row.id).count < Int(clamping: maxItems)
    }

    private var canRemove: Bool {
        guard let minItems = row.field.minItems else { return true }
        return schema.items(row.id).count > Int(clamping: minItems)
    }

    @ViewBuilder private func entry(of item: ListItemDraft, field: FormItemField) -> some View {
        switch field.kind {
        case .boolean:
            Toggle(
                field.title,
                isOn: Binding(
                    get: { value(of: item, field.key) == .toggle(true) },
                    set: { schema.setItemOn(row.id, item: item.id, entry: field.key, $0) }))
        case .choice:
            Picker(
                field.title,
                selection: Binding(
                    get: { textValue(of: item, field.key) },
                    set: { schema.setItemText(row.id, item: item.id, entry: field.key, $0) })
            ) {
                ForEach(field.choices, id: \.self) { Text($0).tag($0) }
            }
        case .text, .integer, .list, .unrecognized:
            TextField(
                field.title,
                text: Binding(
                    get: { textValue(of: item, field.key) },
                    set: { schema.setItemText(row.id, item: item.id, entry: field.key, $0) }))
        }
    }

    private func value(of item: ListItemDraft, _ key: String) -> FieldValue? {
        schema.items(row.id).first { $0.id == item.id }?.entries[key]
    }

    private func textValue(of item: ListItemDraft, _ key: String) -> String {
        if case .text(let text)? = value(of: item, key) { text } else { "" }
    }
}
