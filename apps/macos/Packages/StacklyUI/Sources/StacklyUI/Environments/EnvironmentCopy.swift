import Foundation
import StacklyClient

/// Every user facing string of the Environments page, the plan sheet, and the import
/// (spec 0044, *Layar*). The spec wins over the mockup where they differ.
nonisolated enum EnvironmentCopy {
    /// The schema the app reads.
    static let schema = "stackly/v1alpha1"

    /// "{n} of {m} ready".
    static func readyText(_ summary: EnvironmentSummary) -> String {
        "\(summary.readyCount) of \(summary.serviceCount) ready"
    }

    /// "{n} of {m} ready · {file}", with the home folder written as `~`, or without the file
    /// when the environment has no manifest location.
    static func rowSubtitle(_ summary: EnvironmentSummary, home: String) -> String {
        guard let location = summary.manifestLocation else { return readyText(summary) }
        return "\(readyText(summary)) · \(file(location, home: home))"
    }

    /// The manifest path with the home folder written as `~`.
    static func file(_ location: String, home: String) -> String {
        UserHome.tildePath(location, home: home)
    }

    /// The subtitle of a detail page: the file, or "No manifest file".
    static func detailSubtitle(_ summary: EnvironmentSummary, home: String) -> String {
        summary.manifestLocation.map { file($0, home: home) } ?? "No manifest file"
    }

    // MARK: - Empty and failed

    static let emptyTitle = "No Environments"
    static let emptyMessage = "Import a stackly.yaml to review its plan."
    static let importTitle = "Import…"
    static let loadFailedTitle = "Couldn’t Load Environments"
    static let detailFailedTitle = "Couldn’t Load Environment"
    static let unsupportedTitle = "Environments Unavailable"
    static let unsupportedReason =
        "This version of stacklyd doesn’t manage environments. Update Stackly."
    static let goneTitle = "Environment Not Found"
    static let goneMessage = "This environment was removed. Choose another one in the sidebar."

    // MARK: - Manifest banner

    static let changedTitle = "The manifest changed on disk."
    static let unreadableTitle = "The manifest file can’t be read."

    /// "The manifest has {n} problems.", with "1 problem" for one.
    static func invalidTitle(_ count: UInt64) -> String {
        count == 1 ? "The manifest has 1 problem." : "The manifest has \(count) problems."
    }

    /// "This file now declares “{name}”.", or a general sentence when the name is not known.
    static func nameChangedTitle(_ name: String?) -> String {
        guard let name else { return "This file now declares another environment." }
        return "This file now declares “\(name)”."
    }

    // MARK: - Partially ready

    /// The reason of a service that is not ready: "stopped", "not created", "failed", or the
    /// readiness detail.
    static func notReadyText(_ notReady: ServiceNotReady) -> String {
        switch notReady.reason {
        case .notCreated: return "not created"
        case .stopped: return "stopped"
        case .starting: return "starting"
        case .stopping: return "stopping"
        case .failed: return notReady.detail.map { "failed: \($0)" } ?? "failed"
        case .notReady: return notReady.detail ?? "not ready"
        case .unavailable: return "unavailable"
        case .unrecognized: return notReady.detail ?? "not ready"
        }
    }

    // MARK: - Services table

    /// "Dedicated" or "Shared ref", or "Unknown" for a value this build does not know.
    static func ownershipText(_ ownership: EnvOwnership) -> String {
        switch ownership {
        case .dedicated: "Dedicated"
        case .shared: "Shared ref"
        case .unrecognized: "Unknown"
        }
    }

    /// `{provider}@{selector}` or `instanceRef: {name}`.
    static func selectorText(_ service: EnvironmentService) -> String {
        if let reference = service.instanceRef {
            return "instanceRef: \(reference)"
        }
        return "\(service.provider)@\(service.selector ?? "stable")"
    }

    static let willBeCreated = "Will be created"
    /// The copy item of a service's menu (AC-19).
    static let copyConnectionURL = "Copy Connection URL"

    // MARK: - Validate

    /// "{name}: schema stackly/v1alpha1 valid · no cycles · secrets referenced."
    static func validMessage(_ name: String) -> String {
        "\(name): schema \(schema) valid · no cycles · secrets referenced."
    }

    // MARK: - Remove

    /// "Remove “{name}”?"
    static func removeTitle(_ name: String) -> String {
        "Remove “\(name)”?"
    }

    static let removeMessage =
        "Stackly forgets this environment. Its instances, data, and stackly.yaml are kept."
    static let removeAction = "Remove"
    static let leaseActive = "Bring it down first."

    /// "Removed “{name}”."
    static func removed(_ name: String) -> String {
        "Removed “\(name)”."
    }

    // MARK: - Import

    static let filePanelMessage =
        "Choose a stackly.yaml. You’ll review trust and a plan before anything runs."
    static let fileExtensions = ["yaml", "yml"]

    /// "“{name}” is already registered at {location}."
    static func nameTaken(_ name: String, location: String?, home: String) -> String {
        guard let location else { return "“\(name)” is already registered at another file." }
        return "“\(name)” is already registered at \(file(location, home: home))."
    }

    /// "Imported “{name}”."
    static func imported(_ name: String) -> String {
        "Imported “\(name)”."
    }

    /// "Reloaded “{name}”."
    static func reloaded(_ name: String) -> String {
        "Reloaded “\(name)”."
    }

    // MARK: - Plan sheet

    /// "Plan for {name}".
    static func planTitle(_ name: String) -> String {
        "Plan for \(name)"
    }

    static let planSubtitle = "Review what Up will do. Nothing runs until you confirm."
    static let planFootnote = "Services start in dependency order and wait for readiness."
    static let needsApproval = "Needs approval"
    static let reviewInCatalog = "Review in Catalog"

    /// "Differs from manifest: {field}".
    static func differs(_ field: String) -> String {
        "Differs from manifest: \(field)"
    }

    /// "Uses secret {reference}" or "Needs secret: {reference}"; a generated field has no
    /// reference and no line.
    static func secretLine(_ secret: PlanSecret) -> String? {
        guard let reference = secret.reference else { return nil }
        switch secret.status {
        case .present: return "Uses secret \(reference)"
        case .missing, .unregistered, .unknown, .generated, .unrecognized:
            return "Needs secret: \(reference)"
        }
    }

    /// The port of a create item: the number, or "auto port".
    static func portText(_ port: PortRequest) -> String {
        switch port {
        case .fixed(let number): String(number)
        case .auto: "auto port"
        }
    }
}
