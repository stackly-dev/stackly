import Foundation
import Observation
import StacklyClient

/// The control a settings field is drawn with (spec 0020, *Form dari settings schema*).
nonisolated public enum SchemaControl: Equatable, Sendable {
    /// A secret: Generate, Enter, or Copy from, with the limits of the field.
    case secret(SecretLimits)
    /// A field Stackly sets: a read only row.
    case managed
    /// A text field.
    case text
    /// A whole number.
    case integer
    /// A toggle.
    case toggle
    /// A menu of fixed values.
    case choice([String])
    /// A table of items with Add and Remove.
    case list
    /// A kind this build does not know: a read only row.
    case unrecognized
}

/// One row of the settings form, worked out from a ``FormField`` alone.
nonisolated public struct SchemaRow: Identifiable, Equatable, Sendable {
    /// The field the row shows.
    public let field: FormField
    /// The control.
    public let control: SchemaControl

    /// The key of the field.
    public var id: String { field.key }

    /// Whether the row sits in the closed Advanced group.
    public var isAdvanced: Bool { field.visibility == .advanced }

    /// Whether the field is read only after the instance exists, so the row says "Set once".
    public var isSetOnce: Bool { field.apply == .immutableAfterInit }

    /// Whether the row says "Required" in words: the field is required and has no default.
    public var showsRequired: Bool {
        field.required && field.default == nil && control != .managed
            && control != .unrecognized
    }

    /// The help under a text, integer, or list control: its bounds, or `nil`.
    public var boundsHint: String? {
        switch control {
        case .text:
            lengthHint(minimum: field.minLength, maximum: field.maxLength)
        case .integer:
            switch (field.minimum, field.maximum) {
            case (let minimum?, let maximum?): "From \(minimum) to \(maximum)."
            case (let minimum?, nil): "At least \(minimum)."
            case (nil, let maximum?): "At most \(maximum)."
            case (nil, nil): nil
            }
        case .list:
            switch (field.minItems, field.maxItems) {
            case (let minimum?, let maximum?): "\(minimum) to \(maximum) items."
            case (let minimum?, nil): "At least \(minimum) items."
            case (nil, let maximum?): "At most \(maximum) items."
            case (nil, nil): nil
            }
        default:
            nil
        }
    }

    private func lengthHint(minimum: UInt64?, maximum: UInt64?) -> String? {
        switch (minimum, maximum) {
        case (let minimum?, let maximum?): "\(minimum) to \(maximum) characters."
        case (let minimum?, nil): "At least \(minimum) characters."
        case (nil, let maximum?): "At most \(maximum) characters."
        case (nil, nil): nil
        }
    }

    /// Works the row out from `field`.
    public init(_ field: FormField) {
        self.field = field
        if field.managed {
            control = .managed
        } else if field.secret {
            control = .secret(
                SecretLimits(
                    minLength: field.minLength.map { Int(clamping: $0) },
                    maxLength: field.maxLength.map { Int(clamping: $0) },
                    printableOnly: field.printableOnly))
        } else {
            switch field.kind {
            case .text: control = .text
            case .integer: control = .integer
            case .boolean: control = .toggle
            case .choice: control = .choice(field.choices)
            case .list: control = .list
            case .unrecognized: control = .unrecognized
            }
        }
    }
}

/// What the person typed or chose for a field, apart from a secret.
nonisolated public enum FieldValue: Equatable, Sendable {
    /// Text, a number as typed, or the chosen value.
    case text(String)
    /// A toggle.
    case toggle(Bool)
    /// The items of a list.
    case items([ListItemDraft])
}

/// One item of a list field.
nonisolated public struct ListItemDraft: Identifiable, Equatable, Sendable {
    /// Tells one row from another while the list changes.
    public let id: UUID
    /// The value of each item field, by key: ``FieldValue/text(_:)`` or ``FieldValue/toggle(_:)``.
    public var entries: [String: FieldValue]

    /// Creates an item.
    public init(id: UUID = UUID(), entries: [String: FieldValue]) {
        self.id = id
        self.entries = entries
    }
}

/// What the person chose for a secret field, without the typed value.
/// The typed text stays in the `@State` of the sheet view and nowhere else (spec 0011, *GUI*);
/// the model only needs to know the source and whether what was typed fits the limits.
nonisolated public struct SecretChoice: Equatable, Sendable {
    /// Generate, Enter, or Copy from.
    public var source: SecretDraftSource
    /// The reference to copy from, for Copy from.
    public var reference: String?
    /// Whether the main button may go on: Generate always, Enter when the text fits the
    /// limits, Copy from once a reference is picked.
    public var isValid: Bool

    /// Creates a choice.
    public init(
        source: SecretDraftSource = .generate, reference: String? = nil, isValid: Bool = true
    ) {
        self.source = source
        self.reference = reference
        self.isValid = isValid
    }
}

/// The settings form of the New Instance sheet, built from the `FormField` values of
/// `instance.get_create_form` and nothing else (spec 0020, AC-21): no provider branch.
/// Every mutation tells ``onChange``, so the owner raises the draft generation and runs the
/// preflight again. The model never holds a secret value.
@Observable
public final class SchemaFormModel {
    /// The fields in the order the schema declares them.
    public let fields: [FormField]
    /// One row per field, in the same order.
    public let rows: [SchemaRow]
    /// The value of each field that takes one.
    public private(set) var values: [String: FieldValue]
    /// The choice of each secret field.
    public private(set) var secretChoices: [String: SecretChoice]
    /// The references Copy from offers, the same for every secret field that has no list of
    /// its own in ``copyFromOptionsByField``.
    public var copyFromOptions: [String] = []
    /// The references Copy from offers for a secret field that differs from the rest, by
    /// field key (a duplicate puts the reference of its source first).
    public var copyFromOptionsByField: [String: [String]] = [:]
    /// The labels of the references that have one.
    public var copyFromLabels: [String: String] = [:]
    /// The field issues of the last preflight for the current draft.
    public private(set) var issues: [FieldError] = []

    /// Called after every change the person makes.
    @ObservationIgnored public var onChange: (() -> Void)?

    /// Creates the form with the defaults of the fields.
    /// - Parameter fields: The settings fields of the create form.
    public init(fields: [FormField]) {
        self.fields = fields
        rows = fields.map(SchemaRow.init)
        var values: [String: FieldValue] = [:]
        var choices: [String: SecretChoice] = [:]
        for row in rows {
            switch row.control {
            case .secret: choices[row.id] = SecretChoice()
            case .managed, .unrecognized: break
            case .text, .integer, .choice, .toggle, .list:
                values[row.id] = Self.initialValue(of: row.field)
            }
        }
        self.values = values
        secretChoices = choices
    }

    // MARK: - Rows

    /// The rows outside the Advanced group.
    public var basicRows: [SchemaRow] { rows.filter { !$0.isAdvanced } }

    /// The rows of the Advanced group.
    public var advancedRows: [SchemaRow] { rows.filter(\.isAdvanced) }

    /// The row of a field.
    public func row(_ key: String) -> SchemaRow? {
        rows.first { $0.id == key }
    }

    /// Whether any field of the Advanced group has an issue, which opens the group.
    public var advancedHasIssue: Bool {
        advancedRows.contains { !errorTexts(for: $0.id).isEmpty }
    }

    // MARK: - Reading values

    /// The text of a text, integer, or choice field.
    public func text(_ key: String) -> String {
        if case .text(let text)? = values[key] { text } else { "" }
    }

    /// The state of a toggle field.
    public func isOn(_ key: String) -> Bool {
        if case .toggle(let on)? = values[key] { on } else { false }
    }

    /// The items of a list field.
    public func items(_ key: String) -> [ListItemDraft] {
        if case .items(let items)? = values[key] { items } else { [] }
    }

    /// The choice of a secret field.
    public func secretChoice(_ key: String) -> SecretChoice {
        secretChoices[key] ?? SecretChoice()
    }

    // MARK: - Changing values

    /// Sets the text of a text, integer, or choice field.
    public func setText(_ key: String, _ text: String) {
        guard values[key] != .text(text) else { return }
        values[key] = .text(text)
        onChange?()
    }

    /// Sets a toggle field.
    public func setOn(_ key: String, _ on: Bool) {
        guard values[key] != .toggle(on) else { return }
        values[key] = .toggle(on)
        onChange?()
    }

    /// Adds an item with the defaults of the item fields to a list field, up to `maxItems`.
    public func addItem(to key: String) {
        guard let field = row(key)?.field else { return }
        var items = items(key)
        if let maxItems = field.maxItems, items.count >= Int(clamping: maxItems) { return }
        items.append(ListItemDraft(entries: Self.initialEntries(of: field.items)))
        values[key] = .items(items)
        onChange?()
    }

    /// Removes an item from a list field, down to `minItems`.
    public func deleteItem(from key: String, id: UUID) {
        guard let field = row(key)?.field else { return }
        var items = items(key)
        if let minItems = field.minItems, items.count <= Int(clamping: minItems) { return }
        items.removeAll { $0.id == id }
        values[key] = .items(items)
        onChange?()
    }

    /// Sets the text of one entry of one item.
    public func setItemText(_ key: String, item: UUID, entry: String, _ text: String) {
        updateItem(key, item: item) { $0.entries[entry] = .text(text) }
    }

    /// Sets a toggle entry of one item.
    public func setItemOn(_ key: String, item: UUID, entry: String, _ on: Bool) {
        updateItem(key, item: item) { $0.entries[entry] = .toggle(on) }
    }

    private func updateItem(_ key: String, item: UUID, _ change: (inout ListItemDraft) -> Void) {
        var items = items(key)
        guard let index = items.firstIndex(where: { $0.id == item }) else { return }
        let before = items[index]
        change(&items[index])
        guard items[index] != before else { return }
        values[key] = .items(items)
        onChange?()
    }

    /// Records what the person chose for a secret field.
    /// - Parameters:
    ///   - key: The field.
    ///   - choice: The source, the reference, and whether the typed text fits.
    public func setSecret(_ key: String, _ choice: SecretChoice) {
        guard secretChoices[key] != choice else { return }
        let sourceChanged =
            secretChoices[key].map {
                $0.source != choice.source || $0.reference != choice.reference
            } ?? true
        secretChoices[key] = choice
        // Typing a value changes nothing the preflight sees; its source and reference do.
        if sourceChanged { onChange?() }
    }

    /// Marks every Enter secret as not typed. The view forgets the typed text once Create is
    /// sent, so the model stops claiming the text fits: Create stays off until it is typed again,
    /// while the source and the reference stay what the person chose.
    public func forgetTypedSecrets() {
        for (key, choice) in secretChoices where choice.source == .enter && choice.isValid {
            secretChoices[key]?.isValid = false
        }
    }

    /// The references Copy from offers for a secret field.
    /// - Parameter key: The key of the field.
    public func copyFromOptions(for key: String) -> [String] {
        copyFromOptionsByField[key] ?? copyFromOptions
    }

    // MARK: - Starting from a draft

    /// Fills the fields from a settings document, as a duplicate starts from the settings of
    /// its source. A member the form has no field for, or a value of the wrong shape, is
    /// left out, and the preflight reports what is wrong. It tells nobody: the owner runs
    /// the first preflight itself.
    /// - Parameter settings: The `settings` of a draft.
    public func load(settings: JSONObject) {
        for row in rows {
            guard let member = settings.members[row.id] else { continue }
            switch row.control {
            case .secret, .managed, .unrecognized:
                continue
            case .toggle:
                if case .bool(let on) = member { values[row.id] = .toggle(on) }
            case .text, .integer, .choice:
                if let text = Self.text(of: member) { values[row.id] = .text(text) }
            case .list:
                guard case .array(let elements) = member else { continue }
                values[row.id] = .items(
                    elements.compactMap { Self.item(of: $0, fields: row.field.items) })
            }
        }
    }

    /// Points Copy from of each secret field at a reference while the field still generates.
    /// - Parameter references: The reference for each field key.
    public func preselectCopyFrom(_ references: [String: String]) {
        for (key, reference) in references {
            guard var choice = secretChoices[key] else { continue }
            choice.reference = reference
            secretChoices[key] = choice
        }
    }

    private static func text(of value: JSONValue) -> String? {
        switch value {
        case .string(let text): text
        case .number(let number):
            number.rounded() == number && abs(number) < 9_007_199_254_740_992
                ? String(Int64(number)) : String(number)
        case .null, .bool, .array, .object: nil
        }
    }

    private static func item(of value: JSONValue, fields: [FormItemField]) -> ListItemDraft? {
        guard case .object(let members) = value else { return nil }
        var entries = initialEntries(of: fields)
        for field in fields {
            switch members[field.key] {
            case .bool(let on)?: entries[field.key] = .toggle(on)
            case let member?: if let text = text(of: member) { entries[field.key] = .text(text) }
            case nil: continue
            }
        }
        return ListItemDraft(entries: entries)
    }

    // MARK: - Issues

    /// Replaces the issues with those of the latest preflight.
    public func apply(issues: [FieldError]) {
        self.issues = issues
    }

    /// The sentences under a field, from the issues at its path.
    public func errorTexts(for key: String) -> [String] {
        guard let field = row(key)?.field else { return [] }
        let exact = "settings.\(key)"
        return issues.compactMap { issue in
            guard issue.path == exact || issue.path.hasPrefix("\(exact)[") else { return nil }
            return FieldErrorText.settings(issue, field: field)
        }
    }

    /// The issues of the `settings` object itself and of fields the form does not show.
    public var unmatchedIssues: [FieldError] {
        issues.filter { issue in
            guard issue.path.hasPrefix("settings") else { return false }
            if issue.path == "settings" { return true }
            let key = issue.path.dropFirst("settings.".count)
                .prefix { $0 != "[" }
            return row(String(key)) == nil
        }
    }

    // MARK: - What the draft sends

    /// The settings document of the draft: every field that has a value, without secrets,
    /// managed fields, and fields of a kind this build does not know.
    public func settings() -> JSONObject {
        var members: [String: JSONValue] = [:]
        for row in rows {
            switch row.control {
            case .secret, .managed, .unrecognized:
                continue
            case .toggle:
                members[row.id] = .bool(isOn(row.id))
            case .text, .integer, .choice:
                let text = self.text(row.id)
                guard !text.isEmpty else { continue }
                members[row.id] = Self.scalar(text, integer: row.control == .integer)
            case .list:
                let items = self.items(row.id)
                guard !items.isEmpty || row.field.default != nil else { continue }
                members[row.id] = .array(items.map { Self.object($0, fields: row.field.items) })
            }
        }
        return JSONObject(members)
    }

    /// The source of each secret field, for the preflight. A Copy from without a picked
    /// reference is left out, so the daemon falls back to Generate and the sheet keeps
    /// Create off.
    public func secretPlans() -> [SecretPlan] {
        rows.compactMap { row in
            guard case .secret = row.control else { return nil }
            let choice = secretChoice(row.id)
            switch choice.source {
            case .generate: return SecretPlan(field: row.id, source: .generate)
            case .enter: return SecretPlan(field: row.id, source: .value)
            case .copyFrom:
                return choice.reference.map { SecretPlan(field: row.id, source: .copyFrom($0)) }
            }
        }
    }

    /// Whether every secret field holds a choice that can go on.
    public var secretsAreValid: Bool {
        secretChoices.values.allSatisfy(\.isValid)
    }

    // MARK: - Defaults

    private static func initialValue(of field: FormField) -> FieldValue {
        switch field.default {
        case .text(let text)?: return .text(text)
        case .integer(let number)?: return .text(String(number))
        case .boolean(let on)?: return .toggle(on)
        case .list(let items)?:
            return .items(
                items.map { item in
                    var entries = initialEntries(of: field.items)
                    for entry in item.entries { entries[entry.key] = entryValue(entry.value) }
                    return ListItemDraft(entries: entries)
                })
        case nil:
            switch field.kind {
            case .boolean: return .toggle(false)
            case .list: return .items([])
            case .text, .integer, .choice, .unrecognized: return .text("")
            }
        }
    }

    private static func initialEntries(of fields: [FormItemField]) -> [String: FieldValue] {
        var entries: [String: FieldValue] = [:]
        for field in fields {
            if let value = field.default {
                entries[field.key] = entryValue(value)
            } else {
                entries[field.key] = field.kind == .boolean ? .toggle(false) : .text("")
            }
        }
        return entries
    }

    private static func entryValue(_ value: ScalarSetting) -> FieldValue {
        switch value {
        case .text(let text): .text(text)
        case .integer(let number): .text(String(number))
        case .boolean(let on): .toggle(on)
        }
    }

    private static func scalar(_ text: String, integer: Bool) -> JSONValue {
        if integer, let number = Int64(text.trimmingCharacters(in: .whitespaces)) {
            return .number(Double(number))
        }
        return .string(text)
    }

    private static func object(_ item: ListItemDraft, fields: [FormItemField]) -> JSONValue {
        var members: [String: JSONValue] = [:]
        for field in fields {
            switch item.entries[field.key] {
            case .toggle(let on)?:
                members[field.key] = .bool(on)
            case .text(let text)?:
                guard !text.isEmpty else { continue }
                members[field.key] = scalar(text, integer: field.kind == .integer)
            case .items?, nil:
                continue
            }
        }
        return .object(members)
    }
}
