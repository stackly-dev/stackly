import Foundation
import Observation
import StacklyClient

/// The detail page of one environment (spec 0044, *Layar* and *Event*): the detail from
/// `env.get`, the manifest state, Validate and its Problems, Reload, Locate…, and Remove.
/// `env.get` reads the manifest file only when the page opens, when the app becomes active,
/// after Validate, Reload, or Locate…, and after an operation of this environment; an
/// instance event reads the detail without the file, so the manifest state of the last check
/// stays.
@MainActor
@Observable
final class EnvironmentDetailModel {
    /// The environment.
    let id: UUID
    /// The last detail the daemon sent.
    private(set) var detail: EnvironmentDetail?
    /// The error of the last load, kept next to an older detail.
    private(set) var error: ErrorBody?
    /// Whether a load runs.
    private(set) var isLoading = false
    /// The manifest state of the last check, or `nil` before one.
    private(set) var manifestState: ManifestState?
    /// The error findings of an `invalid` manifest at the last check.
    private(set) var problemCount: UInt64 = 0
    /// The name the file now declares, for the `name_changed` banner.
    private(set) var declaredName: String?
    /// The findings the Problems section lists, or `nil` while it is closed.
    private(set) var findings: [EnvFinding]?
    /// The findings past the limit the daemon sends.
    private(set) var omittedFindings: UInt64 = 0
    /// Whether Validate runs.
    private(set) var isValidating = false
    /// Whether Reload or Locate… runs.
    private(set) var isImporting = false
    /// Whether Remove runs.
    private(set) var isRemoving = false

    @ObservationIgnored private let store: EnvironmentStore
    @ObservationIgnored private let newKey: () -> String
    @ObservationIgnored private var importKey: String?
    @ObservationIgnored private var removeKey: String?

    /// Creates a model; nothing is read until ``load(checkManifest:)``.
    /// - Parameters:
    ///   - id: The environment.
    ///   - store: The environment store, for the reads and the mutations.
    ///   - newKey: Makes an idempotency key.
    init(
        id: UUID, store: EnvironmentStore,
        newKey: @escaping () -> String = { MutationPath.newKey() }
    ) {
        self.id = id
        self.store = store
        self.newKey = newKey
    }

    /// The summary of the last detail, or of the store's list before the first detail.
    var summary: EnvironmentSummary? {
        detail?.summary ?? store.environment(id)
    }

    /// The manifest path, when the environment has one.
    var manifestLocation: String? {
        summary?.manifestLocation
    }

    // MARK: - Reads

    /// Reads `env.get`, with the manifest file when `checkManifest` is true. A read without
    /// the file keeps the manifest state of the last check.
    func load(checkManifest: Bool) async {
        isLoading = true
        defer { isLoading = false }
        do throws(ClientError) {
            let loaded = try await store.detail(id, checkManifest: checkManifest)
            detail = loaded
            error = nil
            if checkManifest {
                manifestState = loaded.manifestState
                problemCount = loaded.problems ?? 0
                if loaded.manifestState == .nameChanged {
                    await readDeclaredName()
                } else {
                    declaredName = nil
                }
            }
        } catch {
            self.error = error.body
        }
    }

    /// The name the file declares now, from `env.validate`.
    private func readDeclaredName() async {
        guard let path = manifestLocation else { return }
        do throws(ClientError) {
            declaredName = try await store.validate(path: path).name
        } catch {
            declaredName = nil
        }
    }

    // MARK: - Validate and Problems

    /// Validate: `env.validate` for the manifest. A valid manifest returns the success
    /// feedback and closes Problems; one that is not valid opens Problems and returns `nil`.
    /// Either way the detail is read again with the file.
    func validate() async -> Feedback? {
        guard let path = manifestLocation, let name = summary?.name else { return nil }
        isValidating = true
        defer { isValidating = false }
        let feedback: Feedback?
        do throws(ClientError) {
            let validation = try await store.validate(path: path)
            if validation.valid {
                findings = nil
                feedback = .success(EnvironmentCopy.validMessage(validation.name ?? name))
            } else {
                findings = validation.findings ?? []
                omittedFindings = validation.omitted
                feedback = nil
            }
        } catch {
            feedback = .failure(error.body)
        }
        await load(checkManifest: true)
        return feedback
    }

    /// Show Problems of the `invalid` banner: the same check as Validate.
    func showProblems() async -> Feedback? {
        await validate()
    }

    /// Closes the Problems section.
    func closeProblems() {
        findings = nil
    }

    // MARK: - Reload and Locate

    /// Reload: imports the manifest file again so the snapshot matches it (spec 0044 *Layar*,
    /// `changed`). A file that is not valid opens Problems instead.
    func reloadManifest() async -> Feedback? {
        guard let path = manifestLocation else { return nil }
        return await importFile(path, relink: false)
    }

    /// Locate…: imports the file the person chose with this environment's ID, so its location
    /// moves there without a new environment (`relinkEnvironmentId`). A file that declares
    /// another name is refused with `env.relink_mismatch`.
    func locate(_ path: String) async -> Feedback? {
        await importFile(path, relink: true)
    }

    private func importFile(_ path: String, relink: Bool) async -> Feedback? {
        isImporting = true
        defer { isImporting = false }
        let validation: EnvValidation
        do throws(ClientError) {
            validation = try await store.validate(path: path)
        } catch {
            return .failure(error.body)
        }
        guard validation.valid, let digest = validation.manifestSha256 else {
            findings = validation.findings ?? []
            omittedFindings = validation.omitted
            await load(checkManifest: true)
            return nil
        }
        let key = importKey ?? newKey()
        importKey = key
        let outcome = await store.send(
            .envImport(
                ImportEnvironmentParams(
                    path: path, expectedManifestSha256: digest,
                    relinkEnvironmentId: relink ? id : nil)), key)
        if case .outcomeUnknown = outcome {} else { importKey = nil }
        var success: Feedback?
        if case .completed(.envImport(let imported)) = outcome {
            detail = imported
            findings = nil
            success = .success(EnvironmentCopy.reloaded(imported.summary.name))
        }
        await store.reload()
        await load(checkManifest: true)
        return Feedback.outcome(outcome, success: success, showOperations: nil)
    }

    // MARK: - Remove

    /// The result of Remove.
    enum RemoveResult {
        /// The environment is gone; the page shows the list.
        case removed(Feedback)
        /// It stays; the feedback says why.
        case kept(Feedback?)
    }

    /// Remove: `env.remove` with the revision the page shows. A lease that is still active is
    /// refused with "Bring it down first." and the instances that hold it.
    func remove() async -> RemoveResult {
        guard let summary else { return .kept(nil) }
        isRemoving = true
        defer { isRemoving = false }
        let key = removeKey ?? newKey()
        removeKey = key
        let outcome = await store.send(
            .envRemove(RemoveEnvironmentParams(id: id, expectedRevision: summary.revision)), key)
        if case .outcomeUnknown = outcome {} else { removeKey = nil }
        switch outcome {
        case .completed(.envRemove(let removed)):
            await store.reload()
            return .removed(.success(EnvironmentCopy.removed(removed.name)))
        case .rejected(let body) where body.code == "env.lease_active":
            return .kept(
                Feedback(
                    tone: .critical, message: EnvironmentCopy.leaseActive,
                    detail: Self.leaseInstances(body), code: body.code))
        default:
            return .kept(Feedback.outcome(outcome, success: nil, showOperations: nil))
        }
    }

    /// The instances of an `env.lease_active` refusal, joined: "pg17-dev, cache".
    static func leaseInstances(_ body: ErrorBody) -> String? {
        let names = (body.items ?? []).compactMap { item in
            item.fields?.first { $0.key == "name" }?.value
        }
        return names.isEmpty ? DaemonText.sentence(body.message) : names.joined(separator: ", ")
    }
}
