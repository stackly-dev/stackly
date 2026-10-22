import Foundation
import StacklyClient

/// The words under a control for a field error (spec 0020, *Field error*, column *Teks
/// GUI*). The `message` of a `FieldError` is a fixed text per code, so the GUI writes the
/// sentence from the code and the bounds of the field.
nonisolated enum FieldErrorText {
    /// The text for a settings field.
    ///
    /// - Parameters:
    ///   - error: The field error.
    ///   - field: The field the path names, or `nil`.
    static func settings(_ error: FieldError, field: FormField?) -> String {
        guard let field else { return fallback }
        switch error.code {
        case "required":
            return "Required."
        case "unknown_value":
            return field.kind == .choice ? "Choose one of the listed values." : "Unknown setting."
        case "invalid_format":
            return "Doesn’t match the required format."
        case "too_short":
            if field.kind == .list, let minItems = field.minItems {
                return "Use at least \(minItems) items."
            }
            return field.minLength.map { "Use at least \($0) characters." } ?? fallback
        case "too_long":
            return field.maxLength.map { "Use at most \($0) characters." } ?? fallback
        case "too_many":
            return field.maxItems.map { "Use at most \($0) items." } ?? fallback
        case "out_of_range":
            return range(minimum: field.minimum, maximum: field.maximum)
        case "invalid_value":
            if field.managed { return "Set by Stackly; leave it out." }
            if let itemKey = field.itemKey, error.path.hasSuffix(".\(itemKey)") {
                return "Each \(itemKey) must be unique."
            }
            return "Use a \(kindWord(field.kind))."
        default:
            return fallback
        }
    }

    /// The text for the name.
    ///
    /// - Parameters:
    ///   - error: The field error at `name`.
    ///   - name: The name of the draft.
    static func name(_ error: FieldError, name: String) -> String {
        switch error.code {
        case "invalid_format":
            "Use 1 to 63 lowercase letters, digits, or hyphens, starting with a letter."
        case "invalid_value":
            "An instance named \(name) already exists."
        default:
            fallback
        }
    }

    private static let fallback = "Check this value."

    private static func range(minimum: Int64?, maximum: Int64?) -> String {
        switch (minimum, maximum) {
        case (let minimum?, let maximum?): "Use a whole number from \(minimum) to \(maximum)."
        case (let minimum?, nil): "Use a whole number of at least \(minimum)."
        case (nil, let maximum?): "Use a whole number of at most \(maximum)."
        case (nil, nil): "Use a whole number."
        }
    }

    private static func kindWord(_ kind: FieldKind) -> String {
        switch kind {
        case .text, .choice: "text"
        case .integer: "whole number"
        case .boolean: "true or false"
        case .list: "list"
        case .unrecognized: "value"
        }
    }
}
