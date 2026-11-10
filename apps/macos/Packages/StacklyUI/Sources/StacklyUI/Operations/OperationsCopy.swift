import Foundation
import StacklyClient

/// Every user facing word of the Operations page and its inspector (spec 0035, *Halaman
/// Operations*), so the page, the fixtures, and the tests read one source.
nonisolated public enum OperationsCopy {
    /// The footer under the list (an adaptation of the mockup copy; its 80% is only an example).
    public static let footer =
        "Stackly stages and checks each change before a step that can’t be undone. From that step on, Cancel is off and the operation finishes on its own."
    /// What the inspector says for an operation the daemon no longer knows (AC-12).
    public static let noLongerInHistory = "This operation is no longer in the history."
    /// The message of an empty page.
    public static let emptyMessage =
        "Installs, starts, stops, and changes show here while they run and after they end."
    /// The reason of an older daemon.
    public static let unsupportedReason =
        "This version of stacklyd doesn’t list operations. Update Stackly."

    /// "{time} · {first 8 characters of the ID}": when it started (or was created, while
    /// queued) and the short ID.
    /// - Parameters:
    ///   - operation: The summary.
    ///   - locale: The locale of the window.
    ///   - timeZone: The time zone of the window.
    public static func meta(
        _ operation: OperationSummary, locale: Locale, timeZone: TimeZone
    ) -> String {
        let style = Date.FormatStyle(
            date: .omitted, time: .shortened, locale: locale, timeZone: timeZone)
        let at = (operation.startedAt ?? operation.createdAt).formatted(style)
        return "\(at) · \(operation.id.uuidString.lowercased().prefix(8))"
    }

    /// A date and time with seconds, for the inspector.
    static func moment(_ date: Date, locale: Locale, timeZone: TimeZone) -> String {
        date.formatted(
            Date.FormatStyle(
                date: .abbreviated, time: .standard, locale: locale, timeZone: timeZone))
    }

    /// The kind as words: `start_instance` is "Start instance".
    public static func kind(_ kind: OperationKind) -> String {
        guard kind != .unrecognized else { return "Unknown" }
        return sentence(kind.rawValue)
    }

    /// Who started it, as the Activity tab says it (spec 0035, *Teks Activity*): "App",
    /// "CLI", "Stackly" for the daemon, or "Client".
    public static func by(_ trigger: OperationTriggerInfo) -> String {
        if trigger.origin == .background { return "Stackly" }
        switch trigger.client {
        case .gui: return "App"
        case .cli: return "CLI"
        case .unrecognized, nil: return "Client"
        }
    }

    /// The resource as words: the instance or runtime name when known, else its kind and ID.
    static func resource(_ resource: ResourceRef?, names: [UUID: String]) -> String {
        guard let resource else { return "Stackly" }
        if let id = UUID(uuidString: resource.id), let name = names[id] { return name }
        let kind = resource.kind == .unrecognized ? "resource" : resource.kind.rawValue
        return "\(sentence(kind)) \(resource.id.prefix(8))"
    }

    /// The state of a step.
    static func step(_ state: StepState) -> String {
        switch state {
        case .running: "Running"
        case .done: "Done"
        case .failed: "Failed"
        case .interrupted: "Interrupted"
        case .unrecognized: "Unknown"
        }
    }

    /// One step: "Done · 2 attempts · 10:04:05 · supervisor.endpoint_in_use".
    static func stepLine(_ step: OperationStepInfo, locale: Locale, timeZone: TimeZone)
        -> String
    {
        var parts = [Self.step(step.state)]
        if step.attempts > 1 { parts.append("\(step.attempts) attempts") }
        let style = Date.FormatStyle(
            date: .omitted, time: .standard, locale: locale, timeZone: timeZone)
        parts.append(step.startedAt.formatted(style))
        if let code = step.errorCode { parts.append(code) }
        return parts.joined(separator: " · ")
    }

    /// The state of something the operation created.
    static func created(_ state: CreatedState) -> String {
        switch state {
        case .intended: "Planned"
        case .created: "Created"
        case .compensated: "Undone"
        case .kept: "Kept"
        case .compensationFailed: "Couldn’t undo"
        case .notOwned: "Not this operation’s"
        case .unrecognized: "Unknown"
        }
    }

    /// The outcome of one item of a partial result.
    static func outcome(_ outcome: PartialOutcome) -> String {
        switch outcome {
        case .done: "Done"
        case .failed: "Failed"
        case .skipped: "Skipped"
        case .leftOver: "Left as it is"
        case .unrecognized: "Unknown"
        }
    }

    /// Why the operation needs recovery.
    static func cause(_ cause: RecoveryCause) -> String {
        switch cause {
        case .interrupted: "It was interrupted"
        case .stepFailed: "A step failed"
        case .compensationFailed: "Undoing it failed"
        case .panicked: "It stopped unexpectedly"
        case .unrecognized: "Unknown"
        }
    }

    /// The recovery actions the daemon offers: "Resume, Abandon", or "None".
    static func actions(_ actions: [RecoveryAction]?) -> String {
        let words = (actions ?? []).map { action in
            switch action {
            case .resume: "Resume"
            case .abandon: "Abandon"
            case .unrecognized: "Other"
            }
        }
        return words.isEmpty ? "None" : words.joined(separator: ", ")
    }

    /// A code as a sentence: underscores become spaces and the first letter is a capital.
    static func sentence(_ code: String) -> String {
        let words = code.replacingOccurrences(of: "_", with: " ")
        return words.prefix(1).uppercased() + words.dropFirst()
    }
}
