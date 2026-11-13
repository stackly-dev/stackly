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
        case .bringingUp:
            return StatusAppearance.upPhase(
                phase: notReady.phase ?? .unrecognized, waitingOn: notReady.waitingOn ?? []
            ).label
        case .needsSecret: return needsSecretText(notReady.references ?? [])
        case .dependencyNotReady:
            return dependencyText(notReady.dependency, reason: notReady.dependencyReason)
        case .upFailed:
            return upFailedText(stage: notReady.stage, code: notReady.code, detail: notReady.detail)
        case .unrecognized: return notReady.detail ?? "not ready"
        }
    }

    /// "Needs secret {reference}", with "+{n}" for more than one (spec 0046, *Layar*).
    static func needsSecretText(_ references: [NeedsSecretItem]) -> String {
        guard let first = references.first else { return "Needs secret" }
        let more = references.count > 1 ? " +\(references.count - 1)" : ""
        return "Needs secret \(first.reference)\(more)"
    }

    /// "Waiting for {dependency}, which failed", "Waiting for {dependency} timed out", or
    /// "Waiting for {dependency}, which is not ready".
    static func dependencyText(_ dependency: String?, reason: DependencyFailure?) -> String {
        let name = dependency ?? "a dependency"
        switch reason {
        case .failed: return "Waiting for \(name), which failed"
        case .timeout: return "Waiting for \(name) timed out"
        case .notReady, .unrecognized, nil: return "Waiting for \(name), which is not ready"
        }
    }

    /// "{Stage} failed: {message}": the stage as `Install`, `Create`, or `Start`, and the
    /// daemon's detail, else the sentence of a known code, else the code.
    static func upFailedText(stage: UpStage?, code: String?, detail: String?) -> String {
        let head: String
        switch stage {
        case .install: head = "Install failed"
        case .create: head = "Create failed"
        case .start: head = "Start failed"
        case .wait, .unrecognized, nil: head = "Up failed"
        }
        guard let message = detail ?? code.map(codeMessage) else { return head }
        return "\(head): \(message)"
    }

    /// A short sentence for an error code an Up reports, or the code itself.
    static func codeMessage(_ code: String) -> String {
        switch code {
        case "supervisor.readiness_timeout": "readiness timed out"
        case "supervisor.endpoint_in_use": "the port is used by another process"
        case "supervisor.endpoint_conflict": "the port is used by another instance"
        case "supervisor.endpoint_unavailable": "the port can’t be used on this Mac"
        case "supervisor.retry_budget_exhausted": "it kept crashing"
        case "supervisor.data_in_use": "its data is in use"
        case "runtime.download_failed": "the download failed"
        case "runtime.checksum_mismatch": "the download did not match its checksum"
        case "provider.approval_required": "the provider needs approval"
        case "resource.busy": "it waited too long for its turn"
        default: code
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
    static let planFootnote =
        "Services start in dependency order and wait for readiness. If one fails, the rest stay up and the environment is marked partially ready."

    // MARK: - Up (spec 0046, *Layar*)

    static let up = "Up…"
    static let bringUp = "Bring Up"
    static let createWithUp = "Create with Up…"
    static let reviewPlan = "Review Plan"
    static let setSecret = "Set…"
    static let resolveBlockers = "Resolve the blockers above first."
    static let planChanged = "The plan changed. Review it again."
    static let alreadyBringingUp = "Already bringing up"
    static let bringingDown = "Bringing down"
    static let noManifest = "This environment has no manifest file."
    static let showInActivity = "Show in Activity"
    static let showOperation = "Show Operation"
    static let setSecretAction = "Set Secret…"
    static let showService = "Show Service"
    static let notReadyTitle = "Not ready"
    static let interrupted = "The last Up was interrupted. Bring it up again to continue."

    /// "Bringing up… {done} of {total}", or "Bringing up…" without a count.
    static func bringingUp(done: UInt64?, total: UInt64?) -> String {
        guard let done, let total, total > 0 else { return "Bringing up…" }
        return "Bringing up… \(done) of \(total)"
    }
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

    // MARK: - Lock and local override (spec 0045, *Layar*)

    /// The text of a row whose value the page does not have: "—".
    static let unknownValue = "—"
    /// The lock file name when the detail does not name one.
    static let defaultLockFile = "stackly.lock"

    /// The Lock row: "{file} · {targets}", "{file} · out of date", "Not locked", or
    /// "{file} · can’t be read"; "—" while the detail carries no lock.
    static func lockText(_ lock: EnvLockInfo?) -> String {
        guard let lock else { return unknownValue }
        switch lock.state {
        case .inSync:
            let targets = lock.targets ?? []
            return targets.isEmpty
                ? lock.file : "\(lock.file) · \(targets.joined(separator: ", "))"
        case .stale: return "\(lock.file) · out of date"
        case .absent: return "Not locked"
        case .invalid, .unreadable: return "\(lock.file) · can’t be read"
        case .unrecognized: return lock.file
        }
    }

    /// The Local override row: "None" without a file or an override, "{service}.{field}:
    /// {value}" and " and {n} more" for more than one, "{file} · {n} problems" for a file with
    /// problems, or "{file} · can’t be read"; "—" while the detail carries no local file.
    static func localText(_ local: EnvLocalInfo?) -> String {
        guard let local else { return unknownValue }
        switch local.state {
        case nil:
            return "None"
        case .present:
            let overrides = local.overrides ?? []
            guard let first = overrides.first else { return "None" }
            let head = overrideText(first)
            return overrides.count > 1 ? "\(head) and \(overrides.count - 1) more" : head
        case .invalid:
            guard local.problems > 0 else { return "\(local.file) · can’t be read" }
            return local.problems == 1
                ? "\(local.file) · 1 problem" : "\(local.file) · \(local.problems) problems"
        case .unreadable:
            return "\(local.file) · can’t be read"
        case .unrecognized:
            return local.file
        }
    }

    /// "{service}.{field}: {value}".
    static func overrideText(_ override: PlanOverride) -> String {
        "\(override.service).\(override.field): \(override.value)"
    }

    static let lockVersions = "Lock Versions"
    static let updateLock = "Update Lock…"
    static let updateLockTitle = "Update the lock?"
    static let updateLockAction = "Update"
    static let copyVariables = "Copy Environment Variables"

    /// "Stackly picks versions again from the catalog and rewrites {file}."
    static func updateLockMessage(_ file: String) -> String {
        "Stackly picks versions again from the catalog and rewrites \(file)."
    }

    /// "Locked {n} services.", with "1 service" for one.
    static func locked(_ count: Int) -> String {
        count == 1 ? "Locked 1 service." : "Locked \(count) services."
    }

    static let lockUpToDate = "The lock is up to date."

    /// The title of the banner that lists the blockers of `env.lock`.
    static func lockBlocked(_ count: Int) -> String {
        count == 1 ? "1 problem blocks the lock." : "\(count) problems block the lock."
    }

    /// "Copied {n} variables.", with "1 variable" for one.
    static func copiedVariables(_ count: Int) -> String {
        count == 1 ? "Copied 1 variable." : "Copied \(count) variables."
    }

    static let nothingToCopy = "Nothing to copy yet. Bring the environment up first."

    // MARK: - Plan sheet, lock and local override (spec 0045)

    /// The end of a create row whose runtime comes from the lock.
    static let lockedSuffix = " · locked"

    /// "Local override: port {port}" or "Local override: data in {path}", or `nil` for a field
    /// this build does not know.
    static func overrideNote(_ override: PlanOverride) -> String? {
        if override.field == "storage.dataLocation" {
            return "Local override: data in \(override.value)"
        }
        if override.field.hasPrefix("endpoints."), override.field.hasSuffix(".port") {
            return "Local override: port \(override.value)"
        }
        return nil
    }

    /// "Differs from lock: {locked version}".
    static func differsFromLock(_ version: String) -> String {
        "Differs from lock: \(version)"
    }

    /// "The lock is out of date for {services}. Those versions come from the catalog."
    static func staleLock(_ services: [String]) -> String {
        "The lock is out of date for \(services.joined(separator: ", ")). Those versions come from the catalog."
    }

    /// The title of the caution that lists the warnings of the local override file.
    static func localWarnings(_ file: String, count: Int) -> String {
        count == 1 ? "\(file) has 1 warning." : "\(file) has \(count) warnings."
    }
}
