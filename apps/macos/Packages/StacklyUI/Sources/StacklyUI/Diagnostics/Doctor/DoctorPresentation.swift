import Foundation
import StacklyClient

/// What the Diagnostics page shows first (spec 0034, AC-20): the findings of one instance, as
/// the instance's failure banner asks, or one check, as the disk space item of Overview asks.
nonisolated public enum DiagnosticsFocus: Hashable, Sendable {
    /// The findings of the instance with this ID, plus every global Error.
    case instance(UUID)
    /// One check.
    case check(DoctorCheckId)
}

/// Where the one safe action of a finding goes (spec 0034, AC-18). Every route is a place to
/// go or a flow that already asks before it changes anything.
nonisolated public enum DoctorActionRoute: Hashable, Sendable {
    /// Open the instance.
    case showInstance(UUID)
    /// Open the Configuration tab of the instance.
    case reviewConfig(UUID)
    /// Open the Configuration tab with the endpoints sheet, the port filled in.
    case changePort(instance: UUID, endpoint: String, port: UInt16)
    /// Open the operation on the Operations page.
    case showOperation(UUID)
    /// Open the Storage tab of Settings.
    case openStorageSettings

    /// The button title (spec 0034, *GUI*).
    public var title: String {
        switch self {
        case .showInstance: "Show Instance"
        case .reviewConfig: "Review"
        case .changePort(_, _, let port): "Use Port \(port)"
        case .showOperation: "Show Operation"
        case .openStorageSettings: "Open Settings"
        }
    }

    /// The route of `action`, or `nil` for an action this build does not know, or one that
    /// lacks what its route needs (an instance, an operation, an endpoint and a port).
    public init?(_ action: DoctorAction) {
        switch action.kind {
        case .showInstance:
            guard let id = action.instanceId else { return nil }
            self = .showInstance(id)
        case .reviewConfig:
            guard let id = action.instanceId else { return nil }
            self = .reviewConfig(id)
        case .changePort:
            guard let id = action.instanceId, let endpoint = action.endpoint,
                let port = action.port
            else { return nil }
            self = .changePort(instance: id, endpoint: endpoint, port: port)
        case .showOperation:
            guard let id = action.operationId else { return nil }
            self = .showOperation(id)
        case .openStorageSettings:
            self = .openStorageSettings
        case .unrecognized:
            return nil
        }
    }
}

/// The routes this build can open, so a route whose page is not there is never offered.
nonisolated public struct DoctorRoutes: Sendable, Equatable {
    /// The kinds of route the shell can open.
    public var available: Set<DoctorActionKind>

    /// Routes of every kind in `available`.
    public init(available: Set<DoctorActionKind>) {
        self.available = available
    }

    /// Every route this build has a page for. `open_storage_settings` is not one: the Settings
    /// window cannot open a chosen tab yet, so that action does not show (spec 0034, AC-18).
    public static let thisBuild = DoctorRoutes(available: [
        .showInstance, .reviewConfig, .changePort, .showOperation,
    ])

    /// Whether `route` can be opened.
    public func contains(_ route: DoctorActionRoute) -> Bool {
        let kind: DoctorActionKind =
            switch route {
            case .showInstance: .showInstance
            case .reviewConfig: .reviewConfig
            case .changePort: .changePort
            case .showOperation: .showOperation
            case .openStorageSettings: .openStorageSettings
            }
        return available.contains(kind)
    }
}

/// One row of the Doctor section: a finding, or a check without findings.
nonisolated public struct DoctorRow: Identifiable, Equatable, Sendable {
    /// What the row's one button does.
    public enum Button: Equatable, Sendable {
        /// The finding's safe action.
        case route(DoctorActionRoute)
        /// Open the detail sheet of the finding, which holds its safe action too.
        case details
    }

    /// A stable key: the place of the check and its ID, then the index of the finding.
    public var id: String
    /// The status the checklist shows.
    public var status: CheckStatus
    /// The title from the daemon.
    public var title: String
    /// The explanation or the summary from the daemon.
    public var detail: String
    /// The one button, or `nil`.
    public var button: Button?
    /// The finding, for the detail sheet; `nil` for a check without findings.
    public var finding: DoctorFinding?
}

/// The rows of the Doctor section, from a report (spec 0034, AC-17, AC-20). A pure function,
/// tested without a view.
nonisolated public enum DoctorPresentation {
    /// The rows of `report`: one per finding (`error` reads failed, `warning` warning, anything
    /// else this build does not know unrecognized) and one passed row per check without
    /// findings, in the daemon's order. A finding with evidence, an excerpt, or a remedy gets
    /// Details…, whose sheet holds its action; any other gets its action when `routes` can open
    /// it.
    ///
    /// A focus on an instance keeps that instance's findings and every global Error; a focus on
    /// a check keeps that check. A focus that leaves nothing shows one passed row.
    public static func rows(
        report: DoctorReport, focus: DiagnosticsFocus?, routes: DoctorRoutes
    ) -> [DoctorRow] {
        var rows: [DoctorRow] = []
        for (place, check) in (report.checks ?? []).enumerated() {
            if case .check(let id) = focus, id != check.id { continue }
            let key = "\(place):\(check.id.rawValue)"
            let findings = check.findings ?? []
            if findings.isEmpty {
                if case .instance = focus { continue }
                rows.append(
                    DoctorRow(
                        id: key, status: .passed, title: check.title,
                        detail: check.summary, button: nil, finding: nil))
                continue
            }
            for (index, finding) in findings.enumerated() {
                if case .instance(let id) = focus {
                    let isTheInstance = finding.instance?.id == id
                    let isGlobalError = finding.instance == nil && finding.severity == .error
                    guard isTheInstance || isGlobalError else { continue }
                }
                rows.append(
                    DoctorRow(
                        id: "\(key).\(index)",
                        status: status(of: finding.severity),
                        title: finding.title, detail: finding.explanation,
                        button: button(for: finding, routes: routes), finding: finding))
            }
            if check.omitted > 0, focus == nil {
                rows.append(
                    DoctorRow(
                        id: "\(key).omitted",
                        status: check.severity == .error ? .failed : .warning,
                        title: "\(check.omitted) more in \(check.title)",
                        detail: "Run stackly doctor --json to see every finding.",
                        button: nil, finding: nil))
            }
        }
        if rows.isEmpty, let focus {
            rows.append(
                DoctorRow(
                    id: "focus", status: .passed, title: "No issues",
                    detail: emptyFocusDetail(focus, report: report), button: nil,
                    finding: nil))
        }
        return rows
    }

    /// The status a finding of `severity` shows.
    public static func status(of severity: DoctorSeverity) -> CheckStatus {
        switch severity {
        case .error: .failed
        case .warning: .warning
        case .ok: .passed
        case .unrecognized: .unrecognized
        }
    }

    /// The one button of `finding`.
    public static func button(for finding: DoctorFinding, routes: DoctorRoutes)
        -> DoctorRow.Button?
    {
        if hasDetails(finding) { return .details }
        guard let action = finding.action, let route = DoctorActionRoute(action),
            routes.contains(route)
        else { return nil }
        return .route(route)
    }

    /// Whether `finding` has more than its explanation: evidence, an excerpt, or a remedy.
    public static func hasDetails(_ finding: DoctorFinding) -> Bool {
        finding.evidence != nil || !(finding.excerpt ?? []).isEmpty || finding.remedy != nil
    }

    /// The issues of `report`: its Error and Warning findings, the ones it left out included.
    public static func issueCount(_ report: DoctorReport) -> UInt64 {
        report.errors + report.warnings
    }

    /// The toolbar subtitle (spec 0034, AC-20): `{n} issues`, `No issues`, or `Not run yet`.
    public static func subtitle(_ report: DoctorReport?) -> String {
        guard let report else { return "Not run yet" }
        switch issueCount(report) {
        case 0: return "No issues"
        case 1: return "1 issue"
        case let count: return "\(count) issues"
        }
    }

    /// The label of the focus, such as `Showing “pg-main”`.
    public static func focusLabel(_ focus: DiagnosticsFocus, report: DoctorReport?) -> String {
        switch focus {
        case .instance(let id):
            let name = instanceName(id, in: report) ?? "this instance"
            return "Showing “\(name)”"
        case .check(let id):
            let title = report?.checks?.first { $0.id == id }?.title ?? "one check"
            return "Showing \(title)"
        }
    }

    private static func emptyFocusDetail(_ focus: DiagnosticsFocus, report: DoctorReport)
        -> String
    {
        switch focus {
        case .instance(let id):
            let name = instanceName(id, in: report) ?? "This instance"
            return "\(name) has no findings, and no global check failed."
        case .check:
            return "This check found nothing to fix."
        }
    }

    private static func instanceName(_ id: UUID, in report: DoctorReport?) -> String? {
        for check in report?.checks ?? [] {
            for finding in check.findings ?? [] where finding.instance?.id == id {
                return finding.instance?.name
            }
        }
        return nil
    }
}
