import Foundation
import StacklyClient

/// One row of the Activity table (spec 0035, *Teks Activity*): the three columns as words, the
/// full date and time for the tooltip, and the operation the row opens.
nonisolated public struct ActivityRow: Identifiable, Equatable, Sendable {
    /// The ID of the audit row.
    public let id: UUID
    /// The Time column: the short time for today, else the short date.
    public let time: String
    /// The full date and time, for the tooltip of the Time column.
    public let fullTime: String
    /// The By column: "App", "CLI", "Stackly", or "Client".
    public let by: String
    /// The Activity column.
    public let activity: String
    /// Whether the row records a failure, so its text takes the critical color.
    public let isFailure: Bool
    /// The operation the row belongs to, which a double click opens (AC-13), or `nil`.
    public let operationId: UUID?
}

/// Every user facing word of the Activity tab (spec 0035, *Teks Activity*, AC-15, AC-18): the
/// columns of a row and the placeholders, so the tab, the fixtures, and the tests read one
/// source. Pure functions of the audit row and the clock. No secret value can show, since the
/// daemon never stores or sends one (spec 0011).
nonisolated public enum ActivityCopy {
    /// The title of a tab with no rows.
    public static let emptyTitle = "No Activity Yet"
    /// The message of a tab with no rows.
    public static let emptyMessage =
        "Creates, starts, stops, configuration changes, and viewed secrets of this instance show here."
    /// The title of a first load that failed.
    public static let failedTitle = "Couldn’t Load Activity"
    /// The title for a daemon without `audit.list`.
    public static let unsupportedTitle = "Activity Unavailable"
    /// The reason for a daemon without `audit.list`.
    public static let unsupportedReason =
        "This version of stacklyd doesn’t keep activity. Update Stackly."
    /// The line under the table.
    public static let footer = "Double-click an entry to open its operation."
    /// The context menu item of a row with an operation.
    public static let showInOperations = "Show in Operations"

    // MARK: - Rows

    /// The rows of `items`, in their order.
    /// - Parameters:
    ///   - items: The audit rows, newest first.
    ///   - now: The moment the tab treats as now.
    ///   - calendar: The calendar of the window.
    ///   - locale: The locale of the window.
    ///   - timeZone: The time zone of the window.
    public static func rows(
        _ items: [AuditEventInfo], now: Date, calendar: Calendar, locale: Locale,
        timeZone: TimeZone
    ) -> [ActivityRow] {
        items.map { item in
            ActivityRow(
                id: item.id,
                time: time(
                    item.occurredAt, now: now, calendar: calendar, locale: locale,
                    timeZone: timeZone),
                fullTime: fullTime(
                    item.occurredAt, calendar: calendar, locale: locale, timeZone: timeZone),
                by: by(item), activity: activity(item),
                isFailure: item.result == .failed || item.result == .needsRecovery,
                operationId: item.operationId)
        }
    }

    // MARK: - Time

    /// The Time column: the short time of the locale for a moment of today, else the short
    /// date without the year, or with it for another year.
    /// - Parameters:
    ///   - date: When it happened.
    ///   - now: The moment the tab treats as now.
    ///   - calendar: The calendar of the window.
    ///   - locale: The locale of the window.
    ///   - timeZone: The time zone of the window.
    public static func time(
        _ date: Date, now: Date, calendar: Calendar, locale: Locale, timeZone: TimeZone
    ) -> String {
        var calendar = calendar
        calendar.timeZone = timeZone
        if calendar.isDate(date, inSameDayAs: now) {
            return date.formatted(
                Date.FormatStyle(
                    date: .omitted, time: .shortened, locale: locale, calendar: calendar,
                    timeZone: timeZone))
        }
        let day = Date.FormatStyle(locale: locale, calendar: calendar, timeZone: timeZone)
            .month(.abbreviated).day()
        let sameYear =
            calendar.component(.year, from: date) == calendar.component(.year, from: now)
        return date.formatted(sameYear ? day : day.year())
    }

    /// The full date and time, for the tooltip of the Time column.
    /// - Parameters:
    ///   - date: When it happened.
    ///   - calendar: The calendar of the window.
    ///   - locale: The locale of the window.
    ///   - timeZone: The time zone of the window.
    public static func fullTime(
        _ date: Date, calendar: Calendar, locale: Locale, timeZone: TimeZone
    ) -> String {
        var calendar = calendar
        calendar.timeZone = timeZone
        return date.formatted(
            Date.FormatStyle(
                date: .long, time: .standard, locale: locale, calendar: calendar,
                timeZone: timeZone))
    }

    // MARK: - By

    /// The By column: "App" for the app, "CLI" for the command line, "Stackly" for what the
    /// daemon did on its own, and "Client" for any other client. The client is what it said in
    /// its handshake, not a checked identity.
    /// - Parameter row: The audit row.
    public static func by(_ row: AuditEventInfo) -> String {
        switch row.client {
        case .gui: return "App"
        case .cli: return "CLI"
        case .unrecognized, nil: break
        }
        return row.actor == .daemon ? "Stackly" : "Client"
    }

    // MARK: - Activity

    /// The Activity column: the text of the action, then " · {errorCode}" for a failure.
    /// - Parameter row: The audit row.
    public static func activity(_ row: AuditEventInfo) -> String {
        let text = base(row)
        if row.result == .failed, let code = detail("errorCode", of: row) {
            return "\(text) · \(code)"
        }
        return text
    }

    private static func base(_ row: AuditEventInfo) -> String {
        let verb = verb(of: row.operationKind ?? .unrecognized)
        switch row.action {
        case "operation.finished":
            return finished(row, verb: verb)
        case "operation.needs_recovery":
            return "\(verb) needs recovery"
        case "operation.cancel":
            return "Cancel requested"
        case "operation.resume":
            return "Resumed"
        case "operation.abandon":
            guard let text = detail("leftovers", of: row), let count = Int(text) else {
                return "Abandoned"
            }
            return "Abandoned, \(count) \(count == 1 ? "item" : "items") left"
        case "secret.reveal":
            let field = field(of: row)
            if detail("purpose", of: row) == "copy" {
                return row.result == .failed ? "Copy \(field) failed" : "Copied \(field)"
            }
            return row.result == .failed ? "View \(field) failed" : "Viewed \(field)"
        case "secret.set":
            let field = field(of: row)
            return row.result == .failed ? "Change \(field) failed" : "Changed \(field)"
        case "secret.delete":
            let field = field(of: row)
            return row.result == .failed ? "Delete \(field) failed" : "Deleted \(field)"
        default:
            return row.action
        }
    }

    /// An `operation.finished` row: a failure or a cancel never uses a template that needs the
    /// details of the job's audit note.
    private static func finished(_ row: AuditEventInfo, verb: String) -> String {
        switch row.result {
        case .failed:
            return detail("reason", of: row) == "abandoned" ? "\(verb) abandoned" : "\(verb) failed"
        case .cancelled:
            return "\(verb) cancelled"
        case .needsRecovery:
            return "\(verb) needs recovery"
        case .unrecognized:
            return "\(verb) ended"
        case .succeeded:
            return succeeded(row, verb: verb)
        }
    }

    /// The text of a finished operation that succeeded, with the fallback in parentheses of
    /// the spec when its detail is missing.
    private static func succeeded(_ row: AuditEventInfo, verb: String) -> String {
        switch row.operationKind ?? .unrecognized {
        case .createInstance:
            return detail("duplicateOfName", of: row).map { "Created from \($0)" } ?? "Created"
        case .startInstance: return "Started"
        case .stopInstance: return "Stopped"
        case .restartInstance: return "Restarted"
        case .reloadInstance: return "Reloaded"
        case .forceStopInstance: return "Force stopped"
        case .changeEndpoints:
            return detail("ports", of: row).map { "Ports changed: \($0)" } ?? "Ports changed"
        case .applyConfig:
            return row.configRevision.map { "Configuration revision \($0) applied" }
                ?? "Configuration applied"
        case .resolveConfigDrift:
            return row.configRevision.map {
                "Configuration files rewritten from revision \($0)"
            } ?? "Configuration files rewritten"
        case .renameInstance:
            guard let old = detail("oldName", of: row), let new = detail("newName", of: row)
            else { return "Renamed" }
            return "Renamed from \(old) to \(new)"
        default:
            return "\(verb) succeeded"
        }
    }

    /// The title of an operation kind without the name of its instance (the words of
    /// ``OperationTitle``), which a failed, cancelled, or other row starts with.
    /// - Parameter kind: The operation kind.
    public static func verb(of kind: OperationKind) -> String {
        switch kind {
        case .installRuntime: "Install runtime"
        case .registerExternalRuntime: "Register external runtime"
        case .removeRuntime: "Remove runtime"
        case .createInstance: "Create"
        case .startInstance: "Start"
        case .stopInstance: "Stop"
        case .restartInstance: "Restart"
        case .reloadInstance: "Reload"
        case .forceStopInstance: "Force stop"
        case .renameInstance: "Rename"
        case .setInstanceStartup: "Change autostart"
        case .stopAllInstances: "Stop all instances"
        case .updateDaemonSettings: "Save settings"
        case .setStorageLocation: "Change storage location"
        case .applyConfig: "Apply configuration"
        case .changeEndpoints: "Change ports"
        case .resolveConfigDrift: "Keep configuration"
        case .removeInstance: "Remove"
        case .purgeInstance: "Purge"
        case .rebindRuntime: "Change runtime"
        case .updateProviderRevision: "Update provider"
        case .backupInstance: "Back up"
        case .restoreInstance: "Restore"
        case .environmentUp: "Bring up environment"
        case .environmentDown: "Bring down environment"
        case .updateManager: "Update Stackly"
        case .migrateData: "Migrate data"
        case .daemonRestart: "Restart stacklyd"
        case .unrecognized: "Operation"
        }
    }

    /// The secret field a row is about: the last part of its `reference` detail in words
    /// ("local:instance/…/superuser-password" is "superuser password"), or "secret".
    private static func field(of row: AuditEventInfo) -> String {
        guard let reference = detail("reference", of: row),
            let last = reference.split(separator: "/").last, last.isEmpty == false
        else { return "secret" }
        return last.replacingOccurrences(of: "-", with: " ")
    }

    /// The value of the detail `key`, or `nil` when the row has none or it is empty.
    private static func detail(_ key: String, of row: AuditEventInfo) -> String? {
        guard let value = row.details?.first(where: { $0.key == key })?.value,
            value.isEmpty == false
        else { return nil }
        return value
    }
}
