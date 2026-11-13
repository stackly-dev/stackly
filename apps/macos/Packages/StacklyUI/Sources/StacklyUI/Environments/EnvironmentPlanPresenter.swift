import Foundation
import Observation
import StacklyClient
import SwiftUI

/// The plan sheet and the import of a manifest (spec 0044, *Layar*, AC-20 and AC-21), one per
/// Environments page. From a detail page the sheet reviews the registered manifest and has
/// only Done. Import… opens the file panel, reads `env.plan` for the file, and the sheet's
/// Import sends `env.import` with the `manifestSha256` the sheet showed; a name registered at
/// another file offers Use This File, which sends the import with `relinkEnvironmentId`.
/// From a detail page the sheet has Cancel and Bring Up (spec 0046, AC-19, AC-21): Bring Up
/// sends `env.up` with the `planDigest` the sheet showed, `env.plan_changed` reads the plan
/// again and says so, and `secret.needs_secret` opens the secret entry sheet, after which
/// the plan is read again.
@MainActor
@Observable
final class EnvironmentPlanPresenter {
    /// What the sheet reads.
    enum Source: Equatable {
        /// The registered manifest of an environment, from its detail page.
        case review(environment: UUID, name: String)
        /// A file the person chose to import.
        case file(String)
    }

    /// Where the sheet is.
    enum Phase: Equatable {
        /// No sheet.
        case idle
        /// The file panel is open.
        case choosing
        /// The daemon reads the plan.
        case loading(Source)
        /// The daemon answered the plan.
        case plan(EnvPlan, Source)
        /// The plan can't be read.
        case failed(ErrorBody, Source)
        /// `env.import` is on its way.
        case importing(EnvPlan, Source)
        /// The import did not go through; the sheet keeps the plan and says why.
        case importFailed(EnvPlan, Source, ErrorBody)
        /// `env.up` is on its way (spec 0046).
        case bringingUp(EnvPlan, Source)
        /// The Up was not accepted; the sheet keeps the plan and says why.
        case upFailed(EnvPlan, Source, ErrorBody)
    }

    /// Where the sheet is.
    private(set) var phase: Phase = .idle
    /// The note above a plan the sheet read again because the daemon's plan changed, or `nil`.
    private(set) var notice: String?
    /// The references the secret entry sheet asks for, or `nil` while it is closed
    /// (spec 0046, AC-21).
    var secretNeeds: [SecretNeed]?

    @ObservationIgnored private let store: EnvironmentStore
    @ObservationIgnored private let picker: any FilePicking
    @ObservationIgnored private let newKey: () -> String
    @ObservationIgnored private var key: String?
    @ObservationIgnored private var upKey: String?
    /// Runs after an import went through, with the detail the daemon sent: the page selects
    /// the environment and shows the feedback.
    @ObservationIgnored var imported: (@MainActor (EnvironmentDetail) -> Void)?
    /// Runs after the daemon accepted an Up, with its reply: the detail page then shows the
    /// progress of the operation.
    @ObservationIgnored var broughtUp: (@MainActor (EnvUpReply) -> Void)?

    /// Creates a presenter.
    /// - Parameters:
    ///   - store: The environment store, for `env.plan`, `env.import`, and the reload after.
    ///   - picker: Asks the person for a file.
    ///   - newKey: Makes an idempotency key.
    init(
        store: EnvironmentStore, picker: any FilePicking = OpenPanelFilePicker(),
        newKey: @escaping () -> String = { MutationPath.newKey() }
    ) {
        self.store = store
        self.picker = picker
        self.newKey = newKey
    }

    // MARK: - State

    /// Whether the sheet shows.
    var isPresented: Bool {
        switch phase {
        case .idle, .choosing: false
        case .loading, .plan, .failed, .importing, .importFailed, .bringingUp, .upFailed: true
        }
    }

    /// The source of the sheet, when one shows.
    var source: Source? {
        switch phase {
        case .idle, .choosing: nil
        case .loading(let source), .plan(_, let source), .failed(_, let source),
            .importing(_, let source), .importFailed(_, let source, _),
            .bringingUp(_, let source), .upFailed(_, let source, _):
            source
        }
    }

    /// The plan the sheet shows, when there is one.
    var plan: EnvPlan? {
        switch phase {
        case .plan(let plan, _), .importing(let plan, _), .importFailed(let plan, _, _),
            .bringingUp(let plan, _), .upFailed(let plan, _, _):
            plan
        case .idle, .choosing, .loading, .failed: nil
        }
    }

    /// The name in the title: the environment's, or the one the file declares.
    var name: String {
        if case .review(_, let name) = source { return name }
        return plan?.validation.name ?? plan?.environment?.name ?? "stackly.yaml"
    }

    /// Whether the sheet came from Import…, so it has Cancel and Import instead of Done.
    var isImport: Bool {
        if case .file = source { return true }
        return false
    }

    /// Why Import can't run for `plan`, or `nil`: a manifest that is not valid, or a name
    /// registered at another file. Other blockers do not stop an import.
    static func importBlockedReason(_ plan: EnvPlan) -> String? {
        if plan.validation.valid == false {
            return "Fix the problems in the manifest first."
        }
        if plan.registration == .nameTaken {
            return "This name is registered at another file."
        }
        guard plan.validation.manifestSha256 != nil else {
            return "The manifest could not be read."
        }
        return nil
    }

    /// Why Bring Up can't run for `plan`, or `nil` (spec 0046, *Layar*): a plan that is not
    /// `applicable`. A reference without a value does not stop it; the daemon answers
    /// `secret.needs_secret` for it.
    static func upBlockedReason(_ plan: EnvPlan) -> String? {
        guard plan.applicable, plan.planDigest != nil else {
            return EnvironmentCopy.resolveBlockers
        }
        return nil
    }

    // MARK: - Actions

    /// Import… as a toolbar or menu action.
    func stacklyAction() -> StacklyAction {
        StacklyAction(
            id: "environment.import", title: EnvironmentCopy.importTitle, mutates: true,
            isBusy: phase == .choosing, perform: { [weak self] in self?.begin() })
    }

    /// Import… as the action of the empty list.
    func stateAction() -> StateAction {
        StateAction(
            title: EnvironmentCopy.importTitle, mutates: true,
            perform: { [weak self] in self?.begin() })
    }

    /// Opens the file panel, then the plan of the chosen file. Cancel in the panel leaves no
    /// sheet.
    func begin() {
        guard phase == .idle else { return }
        phase = .choosing
        Task {
            guard let path = await chooseFile() else {
                if phase == .choosing { phase = .idle }
                return
            }
            await read(.file(path))
        }
    }

    /// Asks the person for a manifest file, for Import… and Locate….
    func chooseFile() async -> String? {
        await picker.chooseFile(
            message: EnvironmentCopy.filePanelMessage,
            extensions: EnvironmentCopy.fileExtensions, startingAt: nil)
    }

    /// Import as New of the `name_changed` banner: the import sheet for the same file.
    func importFile(_ path: String) {
        Task { await read(.file(path)) }
    }

    /// Up…, Review Plan, and Create with Up…: the sheet for the registered manifest of an
    /// environment, with Cancel and Bring Up.
    func review(environment id: UUID, name: String) {
        Task { await read(.review(environment: id, name: name)) }
    }

    /// Reads `env.plan` for `source`; `notice` shows above the plan it reads.
    func read(_ source: Source, notice: String? = nil) async {
        key = nil
        upKey = nil
        self.notice = notice
        phase = .loading(source)
        let envSource: EnvSource =
            switch source {
            case .review(let id, _): .registered(id)
            case .file(let path): .file(path)
            }
        do throws(ClientError) {
            let plan = try await store.plan(envSource)
            guard phase == .loading(source) else { return }
            phase = .plan(plan, source)
        } catch {
            guard phase == .loading(source) else { return }
            phase = .failed(error.body, source)
        }
    }

    /// Import of the sheet, or Use This File (`relink` true) for a name registered at another
    /// file. A refusal or a lost connection keeps the sheet with the reason; a retry after an
    /// unknown outcome reuses the key.
    func importPlan(relink: Bool = false) async {
        let current: (EnvPlan, Source)
        switch phase {
        case .plan(let plan, let source), .importFailed(let plan, let source, _):
            current = (plan, source)
        case .idle, .choosing, .loading, .failed, .importing, .bringingUp, .upFailed:
            return
        }
        let (plan, source) = current
        guard case .file(let path) = source, let digest = plan.validation.manifestSha256,
            plan.validation.valid
        else { return }
        let relinkID = relink ? plan.environment?.id : nil
        guard relink == false || relinkID != nil else { return }
        guard relink || Self.importBlockedReason(plan) == nil else { return }
        let key = self.key ?? newKey()
        self.key = key
        phase = .importing(plan, source)
        let outcome = await store.send(
            .envImport(
                ImportEnvironmentParams(
                    path: path, expectedManifestSha256: digest, relinkEnvironmentId: relinkID)),
            key)
        guard phase == .importing(plan, source) else { return }
        switch outcome {
        case .completed(.envImport(let detail)):
            self.key = nil
            phase = .idle
            await store.reload()
            imported?(detail)
        case .completed:
            self.key = nil
            phase = .importFailed(plan, source, ClientErrorCatalog.protocolViolation())
        case .rejected(let body), .notSent(let body):
            self.key = nil
            phase = .importFailed(plan, source, body)
        case .outcomeUnknown(let body, _):
            phase = .importFailed(plan, source, body)
        }
    }

    /// Bring Up: `env.up` for the registered manifest with the `planDigest` the sheet showed
    /// (spec 0046, AC-19, AC-21). An accepted Up closes the sheet; `env.plan_changed` reads the
    /// plan again with "The plan changed. Review it again."; `secret.needs_secret` opens the
    /// secret entry sheet with its references; any other refusal keeps the sheet with the
    /// reason. A retry after an unknown outcome reuses the key.
    func bringUp() async {
        let current: (EnvPlan, Source)
        switch phase {
        case .plan(let plan, let source), .upFailed(let plan, let source, _):
            current = (plan, source)
        case .idle, .choosing, .loading, .failed, .importing, .importFailed, .bringingUp:
            return
        }
        let (plan, source) = current
        guard case .review(let id, _) = source, Self.upBlockedReason(plan) == nil,
            let digest = plan.planDigest
        else { return }
        let key = upKey ?? newKey()
        upKey = key
        notice = nil
        phase = .bringingUp(plan, source)
        let outcome = await store.send(
            .envUp(
                EnvUpParams(source: .registered(id), expectedPlanDigest: digest, locked: nil)),
            key)
        guard phase == .bringingUp(plan, source) else { return }
        if case .outcomeUnknown = outcome {} else { upKey = nil }
        switch outcome {
        case .completed(.envUp(let reply)):
            phase = .idle
            await store.reload()
            broughtUp?(reply)
        case .completed:
            phase = .upFailed(plan, source, ClientErrorCatalog.protocolViolation())
        case .rejected(let body) where body.code == "env.plan_changed":
            await read(source, notice: EnvironmentCopy.planChanged)
        case .rejected(let body) where body.code == "secret.needs_secret":
            phase = .plan(plan, source)
            let needs = SecretNeed.needs(in: body)
            secretNeeds = needs.isEmpty ? nil : needs
            if needs.isEmpty { phase = .upFailed(plan, source, body) }
        case .rejected(let body), .notSent(let body), .outcomeUnknown(let body, _):
            phase = .upFailed(plan, source, body)
        }
    }

    /// Set… of a "Needs secret: {reference}" line: the secret entry sheet for that reference.
    func setSecret(_ secret: PlanSecret) {
        guard let reference = secret.reference else { return }
        secretNeeds = [
            SecretNeed(reference: reference, reason: .missing, generateAllowed: false)
        ]
    }

    /// The secret entry sheet closed: after Continue the plan is read again, since its
    /// references may have a value now.
    func secretsEntered(saved: Bool) async {
        secretNeeds = nil
        guard saved, let source, case .review = source else { return }
        await read(source)
    }

    /// Cancel: closes the sheet without sending anything. A request on its way is not
    /// cancelled; its answer is dropped.
    func close() {
        key = nil
        upKey = nil
        notice = nil
        secretNeeds = nil
        phase = .idle
    }
}

extension EnvironmentValues {
    /// The plan sheet and import of the Environments page, or `nil` outside it (a preview or a
    /// snapshot of a page alone): then no Import…, Plan…, Review Plan, or Show Plan… shows.
    @Entry var environmentPlan: EnvironmentPlanPresenter? = nil
}
