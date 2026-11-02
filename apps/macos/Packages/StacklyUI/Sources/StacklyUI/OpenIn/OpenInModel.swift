import Foundation
import Observation
import StacklyClient

/// The label of an Open In action (spec 0070, *Label aksi*): "Open in {title}", "Open {title}
/// in Terminal", "Open {title} in Browser", or "Reveal {title} in Finder". The CLI writes the
/// same labels with `open_action_label`; both are held to `detail-open-actions.json`.
public enum OpenActionLabel {
    /// The label of `action`; a kind this build does not know shows its title.
    public static func text(for action: OpenAction) -> String {
        switch action.kind {
        case .app: "Open in \(action.title)"
        case .terminal: "Open \(action.title) in Terminal"
        case .browser: "Open \(action.title) in Browser"
        case .reveal: "Reveal \(action.title) in Finder"
        case .unrecognized: action.title
        }
    }
}

/// What an Open In menu shows (spec 0070, AC-5).
public enum OpenInContent {
    /// No action to show: the button and the submenu are not shown.
    case hidden
    /// The detail is loading: one disabled "Loading…" item.
    case loading
    /// No instance is selected: the submenu is disabled.
    case noSelection
    /// The actions in manifest order, with the `reveal` entries in a section of their own.
    case sections([[StacklyAction]])
}

/// The Open In menu of one window (spec 0070, AC-5 to AC-11): which provider actions show, which
/// are on and why not, and what picking one does. It reads the detail and summary it is given
/// and never keeps a secret value: a password goes from the `secret.reveal` reply straight to
/// ``ConcealedPasteboard``.
@Observable
public final class OpenInModel {
    /// The bundle ID of Terminal, which opens a `.command` script.
    public static let terminalBundleID = "com.apple.Terminal"

    /// "Start the instance to open it."
    static let notRunningText = "Start the instance to open it."
    /// "Not accepting connections yet."
    static let notReadyText = "Not accepting connections yet."
    /// "The file doesn’t exist yet."
    static let fileMissingText = "The file doesn’t exist yet."
    /// "Update Stackly to use this action."
    static let updateText = "Update Stackly to use this action."
    /// "This action is no longer available."
    static let goneText = "This action is no longer available."

    /// Whether an action is being opened.
    public private(set) var isOpening = false

    @ObservationIgnored private let locator: AppLocator
    @ObservationIgnored private let opener: WorkspaceOpener
    @ObservationIgnored private let fileExists: (String) -> Bool
    @ObservationIgnored private let prepareTerminal:
        @Sendable (UUID, String) async throws(ClientError) -> String
    @ObservationIgnored private let secrets: SecretsClient
    @ObservationIgnored private let pasteboard: ConcealedPasteboard

    /// Creates the model.
    /// - Parameters:
    ///   - locator: Finds installed applications.
    ///   - opener: Opens URLs and files.
    ///   - fileExists: Whether a path exists, for `reveal`.
    ///   - prepareTerminal: `instance.prepare_terminal`, returning the script path.
    ///   - secrets: The secret methods, for `secret.reveal { purpose: copy }`.
    ///   - pasteboard: Where a password goes.
    public init(
        locator: AppLocator = WorkspaceAppLocator(),
        opener: WorkspaceOpener = SystemWorkspaceOpener(),
        fileExists: @escaping (String) -> Bool = { FileManager.default.fileExists(atPath: $0) },
        prepareTerminal: @escaping @Sendable (UUID, String) async throws(ClientError) -> String,
        secrets: SecretsClient,
        pasteboard: ConcealedPasteboard = .shared
    ) {
        self.locator = locator
        self.opener = opener
        self.fileExists = fileExists
        self.prepareTerminal = prepareTerminal
        self.secrets = secrets
        self.pasteboard = pasteboard
    }

    // MARK: - Menu

    /// What the menu shows for `summary` and its `detail`.
    /// - Parameters:
    ///   - summary: The instance, or `nil` when none is selected.
    ///   - detail: Its detail, or `nil` while it loads.
    ///   - offlineReason: Why the daemon cannot be reached, which turns every action off.
    ///   - report: Shows the result of an action.
    ///   - enterSecret: Opens the secret entry sheet for a password that is not available.
    ///   - reload: Loads the detail again after the daemon refused a terminal action.
    public func content(
        summary: InstanceSummary?, detail: InstanceDetail?, offlineReason: String? = nil,
        report: @escaping @MainActor (Feedback) -> Void,
        enterSecret: @escaping @MainActor (SecretNeed) -> Void = { _ in },
        reload: @escaping @MainActor () -> Void = {}
    ) -> OpenInContent {
        guard let summary else { return .noSelection }
        guard let detail, detail.instanceId == summary.id else { return .loading }
        let shown = (detail.openActions ?? []).filter(isShown)
        guard shown.isEmpty == false else { return .hidden }
        var main: [StacklyAction] = []
        var reveals: [StacklyAction] = []
        for action in shown {
            let item = StacklyAction(
                id: "openIn.\(action.name)", title: OpenActionLabel.text(for: action),
                mutates: false,
                disabledReason: offlineReason ?? disabledReason(action, summary: summary),
                perform: { [weak self] in
                    guard let self else { return }
                    Task {
                        if let feedback = await self.open(
                            action, summary: summary, detail: detail, enterSecret: enterSecret,
                            reload: reload)
                        {
                            report(feedback)
                        }
                    }
                })
            if action.kind == .reveal {
                reveals.append(item)
            } else {
                main.append(item)
            }
        }
        return .sections([main, reveals].filter { $0.isEmpty == false })
    }

    /// Whether `action` shows at all: an `app` action shows only when one of its bundle IDs is
    /// installed.
    func isShown(_ action: OpenAction) -> Bool {
        guard action.kind == .app else { return true }
        return installedApplication(of: action) != nil
    }

    /// Why `action` is off, or `nil` when it is on (spec 0070, AC-6). A reason of the daemon
    /// wins over the lifecycle.
    func disabledReason(_ action: OpenAction, summary: InstanceSummary) -> String? {
        if action.kind == .unrecognized || action.state == .unrecognized {
            return Self.updateText
        }
        if action.state == .unavailable {
            return Self.reasonText(action.reason)
        }
        switch action.kind {
        case .reveal:
            guard let path = action.path else { return Self.updateText }
            return fileExists(path) ? nil : Self.fileMissingText
        case .app, .browser, .terminal:
            guard summary.lifecycle == .running else { return Self.notRunningText }
            return summary.readiness.state == .ready ? nil : Self.notReadyText
        case .unrecognized:
            return Self.updateText
        }
    }

    /// The text of a reason of the daemon, the same as for a connection string (spec 0021,
    /// *Copy*).
    static func reasonText(_ reason: ConnectionUnavailableReason?) -> String {
        switch reason {
        case .portNotAssigned: "Start the instance to assign its port."
        case .packageUnavailable: DetailFormat.packageUnavailableText
        case .retained: "This instance was removed."
        case .unrecognized, nil: updateText
        }
    }

    /// The text of a `reason` detail of `instance.open_action_unavailable`.
    static func unavailableText(_ reason: String?) -> String {
        switch reason {
        case "not_running": notRunningText
        case "not_ready": notReadyText
        case "retained": reasonText(.retained)
        case "package_unavailable": reasonText(.packageUnavailable)
        case "port_not_assigned": reasonText(.portNotAssigned)
        default: updateText
        }
    }

    private func installedApplication(of action: OpenAction) -> URL? {
        for bundleID in action.appBundleIds ?? [] {
            if let url = locator.applicationURL(forBundleIdentifier: bundleID) {
                return url
            }
        }
        return nil
    }

    // MARK: - Opening

    /// Opens `action` and returns the card to show, or `nil` for a `reveal`, which shows Finder
    /// instead (spec 0070, AC-7 to AC-11).
    public func open(
        _ action: OpenAction, summary: InstanceSummary, detail: InstanceDetail,
        enterSecret: @escaping @MainActor (SecretNeed) -> Void = { _ in },
        reload: @escaping @MainActor () -> Void = {}
    ) async -> Feedback? {
        isOpening = true
        defer { isOpening = false }
        switch action.kind {
        case .app:
            return await openApp(action, summary: summary, detail: detail, enterSecret: enterSecret)
        case .terminal:
            return await openTerminal(
                action, summary: summary, detail: detail, enterSecret: enterSecret, reload: reload)
        case .browser:
            guard let text = action.url, let url = URL(string: text) else {
                return couldNotOpen(action, OpenInFailure.invalidTarget)
            }
            do {
                try opener.openInBrowser(url)
            } catch {
                return couldNotOpen(action, error)
            }
            return .success("Opening \(action.title) for “\(summary.name)” in your browser.")
        case .reveal:
            guard let path = action.path else { return nil }
            opener.reveal(URL(fileURLWithPath: path))
            return nil
        case .unrecognized:
            return nil
        }
    }

    private func openApp(
        _ action: OpenAction, summary: InstanceSummary, detail: InstanceDetail,
        enterSecret: @escaping @MainActor (SecretNeed) -> Void
    ) async -> Feedback {
        guard let application = installedApplication(of: action) else {
            return couldNotOpen(action, OpenInFailure.applicationMissing)
        }
        guard let text = action.url, let url = URL(string: text) else {
            return couldNotOpen(action, OpenInFailure.invalidTarget)
        }
        let copied = await copyPassword(action, detail: detail)
        do {
            try await opener.open(url, withApplicationAt: application)
        } catch {
            return couldNotOpen(action, error)
        }
        return opened(
            "Opening “\(summary.name)” in \(action.title).", title: action.title, copied: copied,
            enterSecret: enterSecret)
    }

    private func openTerminal(
        _ action: OpenAction, summary: InstanceSummary, detail: InstanceDetail,
        enterSecret: @escaping @MainActor (SecretNeed) -> Void,
        reload: @escaping @MainActor () -> Void
    ) async -> Feedback {
        let script: String
        do throws(ClientError) {
            script = try await prepareTerminal(summary.id, action.name)
        } catch {
            reload()
            return prepareFailure(action, error.body)
        }
        guard let terminal = locator.applicationURL(forBundleIdentifier: Self.terminalBundleID)
        else {
            return couldNotOpen(action, OpenInFailure.applicationMissing)
        }
        let copied = await copyPassword(action, detail: detail)
        do {
            try await opener.open(URL(fileURLWithPath: script), withApplicationAt: terminal)
        } catch {
            return couldNotOpen(action, error)
        }
        return opened(
            "Opening \(action.title) for “\(summary.name)” in Terminal.", title: action.title,
            copied: copied, enterSecret: enterSecret)
    }

    /// How the copy of the password went.
    enum Copy: Equatable {
        /// The action copies none.
        case none
        /// The password is on the concealed pasteboard.
        case copied
        /// The password is missing or the Keychain is locked: Enter Secret… can fix it.
        case unavailable(SecretNeed)
        /// The reveal failed for another reason, with its message.
        case failed(String)
    }

    /// Reads the password of `copySecret` once with `secret.reveal { purpose: copy }` and
    /// writes it to the concealed pasteboard. The value lives only in this call.
    func copyPassword(_ action: OpenAction, detail: InstanceDetail) async -> Copy {
        guard let secret = action.copySecret else { return .none }
        if detail.secrets.first(where: { $0.reference == secret.reference })?.status == .missing {
            return .unavailable(Self.need(secret.reference, reason: .missing))
        }
        do throws(ClientError) {
            let revealed = try await secrets.revealSecret(
                reference: secret.reference, purpose: .copy)
            pasteboard.copy(revealed.value)
            return .copied
        } catch {
            switch error.body.code {
            case "secret.not_found":
                return .unavailable(Self.need(secret.reference, reason: .missing))
            case "secret.store_locked":
                return .unavailable(Self.need(secret.reference, reason: .storeLocked))
            default:
                return .failed(error.body.message)
            }
        }
    }

    private static func need(_ reference: String, reason: NeedsSecretReason) -> SecretNeed {
        SecretNeed(reference: reference, reason: reason, generateAllowed: false)
    }

    /// The card after the application opened.
    private func opened(
        _ message: String, title: String, copied: Copy,
        enterSecret: @escaping @MainActor (SecretNeed) -> Void
    ) -> Feedback {
        switch copied {
        case .none:
            return .success(message)
        case .copied:
            return .success(message, detail: "Password copied. Paste it when \(title) asks.")
        case .unavailable(let need):
            return Feedback(
                tone: .caution, message: "Opened without copying the password.",
                detail: "The password isn’t available.",
                action: StacklyAction(
                    id: "openIn.enterSecret", title: "Enter Secret…", mutates: true,
                    perform: { enterSecret(need) }))
        case .failed(let reason):
            return Feedback(
                tone: .caution, message: "Opened without copying the password.", detail: reason)
        }
    }

    private func couldNotOpen(_ action: OpenAction, _ error: any Error) -> Feedback {
        Feedback(
            tone: .critical, message: "Couldn’t open \(action.title).",
            detail: error.localizedDescription)
    }

    private func prepareFailure(_ action: OpenAction, _ body: ErrorBody) -> Feedback {
        let detail: String
        switch body.code {
        case "instance.open_action_unavailable":
            detail = Self.unavailableText(
                body.details?.first(where: { $0.key == "reason" })?.value)
        case "instance.open_action_not_found":
            detail = Self.goneText
        default:
            detail = body.message
        }
        return Feedback(
            tone: .critical, message: "Couldn’t open \(action.title).", detail: detail,
            code: body.code)
    }
}
