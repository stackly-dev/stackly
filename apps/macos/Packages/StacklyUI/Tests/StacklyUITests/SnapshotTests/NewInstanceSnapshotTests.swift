import StacklyClient
import SwiftUI
import Testing

@testable import StacklyUI

/// The New Instance sheet in every variant (spec 0068 AC-8, spec 0020 AC-20 to AC-22): the
/// form with its three preflight shapes, an error in the Advanced group, Copy from, the
/// progress of the create operation, a failed create, and the states without a form.
///
/// The sheet renders at 600 pt and, in the narrow variant, at its minimum of 560 pt, which
/// is the large text check of spec 0012.
extension SnapshotTests {
    struct NewInstanceSnapshotTests {
        /// The sheet these snapshots show, named so the source rule finds its suite.
        private typealias Sheet = NewInstanceSheet

        private static let size = SnapshotSize.fitting(width: 600, narrowWidth: 560)

        private static var isPrimed = false

        /// Renders the sheet once and keeps nothing. The first sheet a process draws looks
        /// different from every later one (its first control shows a focus mark), and the
        /// tests of a suite run in any order, so the one that goes first would differ from its
        /// reference. Priming makes every test see the later look, alone or in the full run.
        @MainActor
        private static func prime() async throws {
            guard !isPrimed else { return }
            isPrimed = true
            let rig = CreateRig()
            await rig.load()
            rig.model.begin(software: "postgresql")
            await settle("the primer form") { rig.model.currentPreflight != nil }
            _ = try await render(
                NewInstanceSheet(model: rig.model, initialDrafts: [:]), variant: .light,
                size: Self.size)
        }

        /// A rig opened on the first runtime and waiting for its first preflight.
        @MainActor
        private func rig(
            runtimes: RuntimeListPage = FixtureRuntime.page,
            software: String? = "postgresql",
            prepare: (CreateRig) -> Void = { _ in }
        ) async -> CreateRig {
            try? await Self.prime()
            let rig = CreateRig(runtimes: runtimes)
            prepare(rig)
            await rig.load()
            rig.model.begin(software: software)
            return rig
        }

        @MainActor
        private func ready(
            _ preflight: CreatePreflight = FixtureCreateForm.ready,
            software: String? = "postgresql"
        ) async -> CreateRig {
            let rig = await rig(software: software) { $0.service.onPreflight = { _ in preflight } }
            await settle("the first preflight") { rig.model.currentPreflight != nil }
            await settle("the Copy from list") { !rig.model.copyFromOptions.isEmpty }
            return rig
        }

        @MainActor
        private func sheet(_ rig: CreateRig, drafts: [String: SecretDraft] = [:]) -> some View {
            NewInstanceSheet(model: rig.model, initialDrafts: drafts)
        }

        // MARK: - The form

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected: "The New Instance form with a ready preflight renders in every variant"
            ))
        func theFormWithAReadyPreflight() async throws {
            let rig = await ready()
            try await assertSnapshot(
                sheet(rig), named: "new-instance-ready", size: Self.size, systemMaterial: true)
        }

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected: "The New Instance form with a taken port renders in every variant"
            ))
        func theFormWithATakenPort() async throws {
            let rig = await ready(FixtureCreate.portConflict)
            try await assertSnapshot(
                sheet(rig), named: "new-instance-port-taken", size: Self.size, systemMaterial: true)
        }

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected: "The New Instance form with a warning renders in every variant"
            ))
        func theFormWithAWarning() async throws {
            let rig = await ready(FixtureCreateForm.caution)
            try await assertSnapshot(
                sheet(rig), named: "new-instance-warning", size: Self.size, systemMaterial: true)
        }

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected:
                    "The New Instance form with an error in the Advanced group renders in every variant"
            ))
        func theFormWithAnErrorInTheAdvancedGroup() async throws {
            let preflight = CreatePreflight(
                ready: false, provider: FixtureCreateForm.ready.provider,
                checks: FixtureCreateForm.ready.checks.map { check in
                    guard check.kind == .data else { return check }
                    return PreflightCheck(
                        id: "settings", kind: .settings, status: .error, title: "Settings",
                        detail: "1 settings need attention", subject: nil, suggestedPort: nil,
                        error: nil)
                },
                fields: [
                    FieldError(
                        path: "settings.maxConnections", code: "out_of_range",
                        message: "value is outside the allowed range")
                ], command: FixtureCreateForm.ready.command)
            let rig = await ready(preflight)
            try await assertSnapshot(
                sheet(rig), named: "new-instance-advanced-error", size: Self.size,
                systemMaterial: true)
        }

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected: "The New Instance form with Copy from chosen renders in every variant"
            ))
        func theFormWithCopyFromChosen() async throws {
            let rig = await ready()
            var draft = SecretDraft()
            draft.source = .copyFrom
            draft.copyFromReference = FixtureCreateForm.copyFromReference
            try await assertSnapshot(
                sheet(rig, drafts: ["superuserPassword": draft]), named: "new-instance-copy-from",
                size: Self.size, systemMaterial: true)
        }

        // MARK: - The preflight card

        /// The Preflight card and the command under it, on the margin of the sheet: the part
        /// of the sheet that a tall form scrolls out of the first screen.
        @MainActor
        private func preflight(_ rig: CreateRig) -> some View {
            VStack(alignment: .leading, spacing: Spacing.s16) {
                PreflightSection(model: rig.model)
                EquivalentCommandRow(model: rig.model)
            }
            .padding(Spacing.pageHorizontal)
        }

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected: "The Preflight card with every row passed renders in every variant"
            ))
        func thePreflightThatPassed() async throws {
            let rig = await ready()
            try await assertSnapshot(
                preflight(rig), named: "new-instance-preflight-ready", size: Self.size)
        }

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected: "The Preflight card with a failed row renders in every variant"
            ))
        func thePreflightWithAFailedRow() async throws {
            let rig = await ready(FixtureCreate.portConflict)
            try await assertSnapshot(
                preflight(rig), named: "new-instance-preflight-failed", size: Self.size)
        }

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected: "The Preflight card with a warning renders in every variant"
            ))
        func thePreflightWithAWarning() async throws {
            let rig = await ready(FixtureCreateForm.caution)
            try await assertSnapshot(
                preflight(rig), named: "new-instance-preflight-warning", size: Self.size)
        }

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected:
                    "The Preflight card with a status this build does not know renders in every variant"
            ))
        func thePreflightWithAnUnknownStatus() async throws {
            let unknown = CreatePreflight(
                ready: true, provider: FixtureCreateForm.ready.provider,
                checks: FixtureCreateForm.ready.checks.map { check in
                    guard check.kind == .disk else { return check }
                    return PreflightCheck(
                        id: check.id, kind: check.kind, status: .unrecognized,
                        title: check.title, detail: check.detail, subject: nil,
                        suggestedPort: nil, error: nil)
                }, fields: [], command: FixtureCreateForm.ready.command)
            let rig = await ready(unknown)
            try await assertSnapshot(
                preflight(rig), named: "new-instance-preflight-unrecognized", size: Self.size)
        }

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected: "The Preflight card before the first reply renders in every variant"
            ))
        func thePreflightBeforeTheFirstReply() async throws {
            let gate = Gate<CreatePreflight>()
            let rig = await rig { $0.service.onPreflight = { _ in await gate.wait() } }
            await settle("the preflight to wait") { gate.waiting == 1 }
            try await assertSnapshot(
                preflight(rig), named: "new-instance-preflight-checking", size: Self.size)
            gate.release(FixtureCreateForm.ready)
        }

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected: "The Preflight card that could not run renders in every variant"
            ))
        func thePreflightThatCouldNotRun() async throws {
            let rig = await rig {
                $0.service.onPreflight = { _ throws(ClientError) in
                    throw .rejected(.create("daemon.starting", "the daemon is starting"))
                }
            }
            await settle("the failure") { rig.model.preflightFailure != nil }
            try await assertSnapshot(
                preflight(rig), named: "new-instance-preflight-not-run", size: Self.size)
        }

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected: "The Preflight card once the secrets are stored renders in every variant"
            ))
        func thePreflightOnceTheSecretsAreStored() async throws {
            let rig = await ready()
            await rig.model.create(secrets: [])
            rig.emit(CreateRig.operation(state: .running, stage: "layout", revision: 2))
            await settle("the secrets") { rig.model.secretsAreStored }
            try await assertSnapshot(
                preflight(rig), named: "new-instance-preflight-stored", size: Self.size)
        }

        // MARK: - The operation

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected:
                    "The New Instance sheet while the instance is created renders in every variant"
            ))
        func theSheetWhileCreating() async throws {
            let rig = await ready()
            await rig.model.create(secrets: [])
            rig.emit(CreateRig.operation(state: .running, stage: "layout", revision: 2))
            await settle("the stage") { rig.model.operation?.stage == "layout" }
            try await assertSnapshot(
                sheet(rig), named: "new-instance-creating", size: Self.size, systemMaterial: true)
        }

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected: "The New Instance sheet after a failed create renders in every variant"
            ))
        func theSheetAfterAFailedCreate() async throws {
            let rig = await ready()
            await rig.model.create(secrets: [])
            rig.emit(
                CreateRig.operation(
                    state: .failed, stage: "secrets",
                    failure: OperationFailureInfo(
                        reason: "step_failed", retryable: true,
                        error: .create(
                            "secret.store_locked", "the Keychain is locked; unlock it and try again"
                        )),
                    revision: 2))
            await settle("the form") { rig.model.phase == .editing }
            await settle("the preflight") { rig.model.currentPreflight != nil }
            try await assertSnapshot(
                sheet(rig), named: "new-instance-failed", size: Self.size, systemMaterial: true)
        }

        // MARK: - Without a form

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected: "The New Instance sheet without a runtime renders in every variant"
            ))
        func theSheetWithoutARuntime() async throws {
            let rig = await rig(runtimes: RuntimeListPage(items: [], next: nil, installing: []))
            try await assertSnapshot(
                sheet(rig), named: "new-instance-no-runtimes", size: Self.size, systemMaterial: true
            )
        }

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected: "The New Instance sheet with an empty category renders in every variant"
            ))
        func theSheetWithAnEmptyCategory() async throws {
            let rig = await ready()
            rig.model.selectCategory(.cache)
            try await assertSnapshot(
                sheet(rig), named: "new-instance-empty-category", size: Self.size,
                systemMaterial: true)
        }

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected:
                    "The New Instance sheet whose form failed to load renders in every variant"
            ))
        func theSheetWhoseFormFailed() async throws {
            let rig = await rig {
                $0.service.onForm = { _ throws(ClientError) in
                    throw .rejected(.create("catalog.unavailable", "the catalog is not loaded"))
                }
            }
            await settle("the failure") {
                if case .formFailed = rig.model.phase { true } else { false }
            }
            try await assertSnapshot(
                sheet(rig), named: "new-instance-form-failed", size: Self.size, systemMaterial: true
            )
        }

        @Test(
            .evidence(
                covers: ["NFR-012"],
                expected: "The New Instance sheet while the form loads renders in every variant"
            ))
        func theSheetWhileTheFormLoads() async throws {
            let gate = Gate<CreateForm>()
            let rig = await rig { $0.service.onForm = { _ in await gate.wait() } }
            await settle("the form read") { gate.waiting == 1 }
            try await assertSnapshot(
                sheet(rig), named: "new-instance-loading", size: Self.size, systemMaterial: true)
            gate.release(FixtureCreateForm.database)
        }
    }
}
