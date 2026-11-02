import Foundation
import StacklyClient

/// The words of the Configuration tab, the Rename sheet, and the Ports sheet (spec 0029, *Copy*).
nonisolated public enum ConfigurationCopy {
    /// The tag next to "Revision N": what Apply would do now.
    public static func impactTag(_ impact: ConfigImpact) -> String {
        switch impact {
        case .none, .unrecognized: "In sync"
        case .nextStart: "Applies at next start"
        case .reload: "Applies on reload"
        case .restart: "Requires restart"
        }
    }

    /// The title of the Apply button for an impact, before it runs.
    public static func applyTitle(_ impact: ConfigImpact) -> String {
        switch impact {
        case .reload: "Apply and Reload"
        case .restart: "Apply and Restart…"
        case .none, .nextStart, .unrecognized: "Apply"
        }
    }

    /// The busy label of the Apply button.
    public static let applying = "Applying…"
    /// The busy label of Roll Back.
    public static let rollingBack = "Rolling Back…"
    /// The busy label of Keep Revision.
    public static let keepingRevision = "Keeping Revision…"
    /// The busy label of the Rename sheet.
    public static let renaming = "Renaming…"
    /// The busy label of the Ports sheet.
    public static let changingPorts = "Changing Ports…"

    /// What the pending changes list says when there are none.
    public static let noPendingChanges = "No pending changes. Edit a value to see the diff."

    /// The line under a port row of the form.
    public static let portRow = "Managed by Stackly · change in Endpoints"

    /// How a revision came to be, for a row of History.
    public static func origin(_ revision: ConfigRevisionSummary) -> String {
        let source = revision.sourceNumber.map(String.init)
        switch revision.origin {
        case .created: return "Created from provider defaults"
        case .applied: return "Applied"
        case .endpoints: return "Ports changed"
        case .rollback:
            return source.map { "Rolled back to revision \($0)" } ?? "Rolled back"
        case .restored:
            return source.map { "Restored from revision \($0)" } ?? "Restored"
        case .unrecognized: return "Changed"
        }
    }

    /// The line under a field that can be edited: when its change applies, and its default.
    /// - Parameters:
    ///   - field: The field.
    ///   - reloadSupported: Whether the provider can reload; a `reload` field of a provider
    ///     that cannot is a restart in practice.
    /// - Returns: The line, or `nil` when the field says neither.
    public static func fieldNote(_ field: FormField, reloadSupported: Bool) -> String? {
        let applies: String?
        switch field.apply {
        case .reload?: applies = reloadSupported ? "Applies on reload" : "Requires restart"
        case .restart?: applies = "Requires restart"
        case .immutableAfterInit?, .unrecognized?, nil: applies = nil
        }
        let fallback = defaultText(field.default).map { "default \($0)" }
        switch (applies, fallback) {
        case (let applies?, let fallback?): return "\(applies) · \(fallback)"
        case (let applies?, nil): return applies
        case (nil, let fallback?): return fallback.prefix(1).uppercased() + fallback.dropFirst()
        case (nil, nil): return nil
        }
    }

    /// The default of a field as text, or `nil` for a list or no default.
    public static func defaultText(_ value: SettingValue?) -> String? {
        switch value {
        case .text(let text)?: text.isEmpty ? nil : text
        case .integer(let number)?: String(number)
        case .boolean(let on)?: on ? "on" : "off"
        case .list?, nil: nil
        }
    }

    /// The title of the drift banner: the first file, and how many more.
    public static func driftTitle(_ drift: ConfigDrift) -> String {
        guard let first = drift.files.first else { return "Configuration changed outside Stackly" }
        let more = drift.files.count - 1
        let name = more > 0 ? "\(first.path) and \(more) more" : first.path
        return "\(name) was changed outside Stackly"
    }

    /// The message of the drift banner.
    public static func driftMessage(_ drift: ConfigDrift, name: String, locale: Locale) -> String {
        let time = drift.observedAt.formatted(
            Date.FormatStyle(date: .abbreviated, time: .shortened).locale(locale))
        return "\(time) · Stackly won’t overwrite it or start \(name) until you choose."
    }

    /// The title of the roll back banner.
    public static func rollBackTitle(active: UInt64) -> String {
        "Revision \(active) hasn’t reached ready"
    }

    /// The message of the roll back banner.
    public static func rollBackMessage(lastKnownGood: UInt64) -> String {
        "Revision \(lastKnownGood) is the last known good configuration."
    }

    /// The card after an apply that finished.
    public static func applied(revision: UInt64, impact: ConfigImpact, name: String) -> String {
        impact == .nextStart
            ? "Revision \(revision) saved. It applies at next start."
            : "Revision \(revision) applied to \(name)."
    }

    /// The text of a change in the pending list: `key = value`, or `key` unset.
    public static func settingLine(key: String, value: String?) -> String {
        guard let value else { return "\(key) unset" }
        return "\(key) = \(value)"
    }

    /// The text of a port in the pending list.
    public static func portText(_ request: PortRequest) -> String {
        switch request {
        case .fixed(let port): String(port)
        case .auto: "Automatic"
        }
    }
}
