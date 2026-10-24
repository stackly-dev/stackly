import StacklyClient
import SwiftUI

extension SchemaRow {

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

                .onChange(of: schema.advancedHasIssue, initial: true) { _, hasIssue in
                    if hasIssue { advancedExpanded = true }
                }
            }
        }
    }

    /// The draft a secret row starts from: Generate, with Copy from pointing at the reference
    /// the form offers first (the reference the source of a duplicate uses), or at none.
    private func startingDraft(of row: SchemaRow) -> SecretDraft {
        var draft = SecretDraft()
        draft.copyFromReference = schema.secretChoice(row.id).reference
        return draft
    }

    /// The draft of a secret row. Setting it also tells the model the source and whether the
    /// typed text fits, never the text.
    private func draft(_ row: SchemaRow) -> Binding<SecretDraft> {
        Binding(
            get: { drafts[row.id] ?? startingDraft(of: row) },
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
                copyFromOptions: schema.copyFromOptions(for: row.id),
                copyFromLabels: schema.copyFromLabels)
        case .text:
            TextField(title, text: text)

        case .integer:
            TextField(title, text: text)
                .multilineTextAlignment(.trailing)
                .monospacedDigit()

        case .toggle:
            Toggle(title, isOn: toggle)

        case .choice(let values):
            Picker(title, selection: text) {
                if text.wrappedValue.isEmpty || !values.contains(text.wrappedValue) {
                    Text("Choose…").tag(text.wrappedValue)
                }
                ForEach(values, id: \.self) { Text($0).tag($0) }
            }
            .pickerStyle(.menu)

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

                    .disabled(!canRemove)
                }
            }
            Button("Add") { schema.addItem(to: row.id) }

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
