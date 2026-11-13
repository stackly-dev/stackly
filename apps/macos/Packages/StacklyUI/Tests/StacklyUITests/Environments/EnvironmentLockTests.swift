import Foundation
import StacklyClient
import StacklyTestSupport
import Testing

@testable import StacklyUI

/// The lock and local override of the app (spec 0045, AC-18, AC-19): the Lock and Local
/// override rows, Lock Versions, Update Lock…, Copy Environment Variables, and the plan sheet
/// lines for the lock and the overrides.
@MainActor
struct EnvironmentLockTests {
    let rig = PageRig(environments: FixtureEnvironments.summaries)

    private func model(_ id: UUID = FixtureEnvironments.erpID) -> EnvironmentDetailModel {
        EnvironmentDetailModel(
            id: id, store: rig.environments, newKey: { "key-1" },
            copyText: { _ in })
    }

    // MARK: - Rows

    @Test(
        .evidence(
            covers: ["NFR-012"],
            expected:
                "The Lock row reads its file and targets, out of date, Not locked, can’t be read, or — without a lock"
        ))
    func theLockRowFollowsTheSpecText() {
        let lock = { (state: LockState, targets: [String]) in
            FixtureEnvironments.lockInfo(state, targets: targets)
        }
        #expect(
            EnvironmentCopy.lockText(lock(.inSync, ["macos-aarch64"]))
                == "stackly.lock · macos-aarch64")
        #expect(
            EnvironmentCopy.lockText(lock(.inSync, ["macos-aarch64", "macos-x86_64"]))
                == "stackly.lock · macos-aarch64, macos-x86_64")
        #expect(EnvironmentCopy.lockText(lock(.stale, [])) == "stackly.lock · out of date")
        #expect(EnvironmentCopy.lockText(lock(.absent, [])) == "Not locked")
        #expect(EnvironmentCopy.lockText(lock(.invalid, [])) == "stackly.lock · can’t be read")
        #expect(
            EnvironmentCopy.lockText(lock(.unreadable, [])) == "stackly.lock · can’t be read")
        #expect(EnvironmentCopy.lockText(nil) == "—")
    }

    @Test(
        .evidence(
            covers: ["NFR-012"],
            expected:
                "The Local override row reads None without a file, the first override and how many more, the problems, or can’t be read"
        ))
    func theLocalOverrideRowFollowsTheSpecText() {
        let local = { (state: LocalState?, overrides: [PlanOverride], problems: UInt64) in
            FixtureEnvironments.localInfo(state, overrides: overrides, problems: problems)
        }
        #expect(EnvironmentCopy.localText(local(nil, [], 0)) == "None")
        #expect(EnvironmentCopy.localText(local(.present, [], 0)) == "None")
        #expect(
            EnvironmentCopy.localText(local(.present, [FixtureEnvironments.portOverride], 0))
                == "postgres.endpoints.client.port: 5434")
        #expect(
            EnvironmentCopy.localText(
                local(
                    .present,
                    [FixtureEnvironments.portOverride, FixtureEnvironments.storageOverride], 0))
                == "postgres.endpoints.client.port: 5434 and 1 more")
        #expect(
            EnvironmentCopy.localText(local(.invalid, [], 2)) == ".stackly.local.yaml · 2 problems")
        #expect(
            EnvironmentCopy.localText(local(.invalid, [], 1)) == ".stackly.local.yaml · 1 problem")
        #expect(
            EnvironmentCopy.localText(local(.unreadable, [], 0))
                == ".stackly.local.yaml · can’t be read")
        #expect(EnvironmentCopy.localText(nil) == "—")
    }

    @Test func aLockIsNeverShownAgainstAManifestThatIsNotValid() async {
        let model = model()
        await model.load(checkManifest: true)
        #expect(model.lock?.state == .inSync)
        #expect(model.local?.overrides?.count == 2)

        // A detail read without the file keeps the rows of the last check.
        await model.load(checkManifest: false)
        #expect(model.lock?.state == .inSync)

        // Even when a daemon sends a readable lock with a manifest that is not valid, the
        // page shows "—".
        rig.script.environmentDetails[FixtureEnvironments.erpID] = FixtureEnvironments.detail(
            FixtureEnvironments.erpID, manifest: .invalid,
            files: (FixtureEnvironments.lockInfo(.inSync), FixtureEnvironments.localInfo(nil)))
        await model.load(checkManifest: true)
        #expect(model.lock == nil)
        #expect(model.local == nil)
        #expect(EnvironmentCopy.lockText(model.lock) == "—")
    }

    // MARK: - Lock Versions and Update Lock…

    @Test(
        .evidence(
            covers: ["REQ-023"],
            expected:
                "Lock Versions sends env.lock with refresh none, Update Lock… with all, and the feedback says Locked 3 services. or The lock is up to date."
        ))
    func lockVersionsAndUpdateLockSendTheirRefresh() async throws {
        let model = model()
        await model.load(checkManifest: true)
        #expect(await model.lockVersions(refresh: .none)?.message == "Locked 3 services.")
        rig.script.lockResult = FixtureEnvironments.lockResult(written: false)
        #expect(await model.lockVersions(refresh: .all)?.message == "The lock is up to date.")
        let refreshes = rig.script.lockCalls.compactMap { call -> LockRefresh? in
            guard case .envLock(let params) = call else { return nil }
            #expect(params.source == .registered(FixtureEnvironments.erpID))
            return params.refresh
        }
        #expect(refreshes == [.none, .all])
        #expect(model.lockBlockers == nil)
        #expect(rig.script.environmentSent.isEmpty)
    }

    @Test func aBlockedLockListsItsBlockersAndWritesNothing() async {
        rig.script.lockResult = FixtureEnvironments.lockBlocked()
        let model = model()
        await model.load(checkManifest: true)
        #expect(await model.lockVersions(refresh: .none) == nil)
        #expect(model.lockBlockers?.map(\.error.code) == ["provider.approval_required"])
        let items = EnvironmentPlanContent.items(model.lockBlockers ?? [])
        #expect(
            items.map(\.reason) == [
                "Provider acme/search is not approved. Review the provider in the Catalog."
            ])
        #expect(EnvironmentCopy.lockBlocked(1) == "1 problem blocks the lock.")
        model.closeLockBlockers()
        #expect(model.lockBlockers == nil)
    }

    @Test func aLockOfAManifestThatIsNotValidOpensProblems() async {
        rig.script.lockResult = EnvLockResult(
            validation: FixtureEnvironments.validation(
                path: FixtureEnvironments.manifest("erp"), valid: false),
            lockFile: "/tmp/stackly.lock", written: false, changed: false, sha256: nil,
            targets: [], services: [], blockers: [])
        let model = model()
        await model.load(checkManifest: true)
        #expect(await model.lockVersions(refresh: .none) == nil)
        #expect(model.findings?.map(\.code) == ["dependency_cycle", "secret_literal"])
    }

    @Test func aFailedLockSaysWhy() async {
        rig.script.lockFailure = .rejected(ClientErrorCatalog.notConnected())
        let model = model()
        await model.load(checkManifest: true)
        let feedback = await model.lockVersions(refresh: .none)
        #expect(feedback?.tone == .critical)
        #expect(model.isLocking == false)
    }

    @Test func theLockItemsAreGatedAndUpdateLockConfirms() throws {
        var runs: [String] = []
        let section = EnvironmentFileActions.lockSection(
            location: FixtureEnvironments.manifest("erp"), lockFile: "stackly.lock",
            isLocking: false, isExporting: false, lock: { runs.append("lock") },
            update: { runs.append("update") }, copy: { runs.append("copy") })
        #expect(
            section.map(\.title) == [
                "Lock Versions", "Update Lock…", "Copy Environment Variables",
            ])
        #expect(section.map(\.mutates) == [true, true, false])
        let confirmation = try #require(section[1].confirmation)
        #expect(confirmation.title == "Update the lock?")
        #expect(
            confirmation.message
                == "Stackly picks versions again from the catalog and rewrites stackly.lock.")
        #expect(confirmation.actionTitle == "Update")

        // Offline the two lock items are off; Copy still runs and reports its failure.
        let offline = MutationAvailability.blocked(
            reason: "stacklyd is unavailable", since: FixtureClock.now)
        #expect(section[0].availability(gate: offline) != .enabled)
        #expect(section[1].availability(gate: offline) != .enabled)
        #expect(section[2].availability(gate: offline) == .enabled)
        var presented: [ConfirmationRequest] = []
        section[1].invoke(gate: .available, present: { presented.append($0) })
        #expect(runs.isEmpty)
        presented.first?.perform()
        section[0].invoke(gate: .available, present: { _ in })
        #expect(runs == ["update", "lock"])

        let missing = EnvironmentFileActions.lockSection(
            location: nil, lockFile: "stackly.lock", isLocking: false, isExporting: false,
            lock: {}, update: {}, copy: {})
        #expect(
            missing.map(\.disabledReason) == [
                "This environment has no manifest file.", "This environment has no manifest file.",
                nil,
            ])
    }

    @Test func theMoreMenuPutsTheLockItemsAfterReloadManifest() {
        let lock = EnvironmentFileActions.lockSection(
            location: FixtureEnvironments.manifest("erp"), lockFile: "stackly.lock",
            isLocking: false, isExporting: false, lock: {}, update: {}, copy: {})
        let sections = EnvironmentFileActions.sections(
            location: FixtureEnvironments.manifest("erp"),
            ports: ManifestFilePorts(open: { _ in }, reveal: { _ in }), reload: {}, lock: lock,
            remove: StacklyAction(
                id: "environment.remove", title: "Remove Environment…", mutates: true,
                perform: {}), isReloading: false)
        #expect(
            sections.map { $0.map(\.title) } == [
                ["Open Manifest", "Reveal in Finder", "Copy Path", "Reload Manifest"],
                ["Lock Versions", "Update Lock…", "Copy Environment Variables"],
                ["Remove Environment…"],
            ])
    }

    // MARK: - Copy Environment Variables

    @Test(
        .evidence(
            covers: ["NFR-012"],
            expected:
                "Copy Environment Variables writes exactly the dotenv text of env.export and says Copied 3 variables.; with no variable it copies nothing"
        ))
    func copyEnvironmentVariablesWritesTheExportText() async {
        var copied: [String] = []
        let model = EnvironmentDetailModel(
            id: FixtureEnvironments.erpID, store: rig.environments,
            copyText: { copied.append($0) })
        #expect(await model.copyVariables().message == "Copied 3 variables.")
        #expect(copied == [FixtureEnvironments.export().text])
        guard case .envExport(let params) = rig.script.lockCalls.last else {
            Issue.record("no env.export")
            return
        }
        #expect(params.format == .dotenv)
        #expect(params.source == .registered(FixtureEnvironments.erpID))

        rig.script.export = FixtureEnvironments.export(empty: true)
        #expect(
            await model.copyVariables().message
                == "Nothing to copy yet. Bring the environment up first.")
        #expect(copied.count == 1)

        rig.script.lockFailure = .notSent(ClientErrorCatalog.notConnected())
        #expect(await model.copyVariables().tone == .critical)
        #expect(copied.count == 1)
    }

    // MARK: - Plan sheet

    @Test(
        .evidence(
            covers: ["REQ-023", "NFR-012"],
            expected:
                "The plan sheet marks a create from the lock with · locked, shows each applied override, the lock difference, and a caution for a stale lock"
        ))
    func thePlanSheetShowsTheLockAndTheOverrides() throws {
        let plan = FixtureEnvironments.lockedPlan()
        let items = try #require(plan.items)
        let name: (String) -> String = { $0 }
        let mysql = try #require(items.first { $0.alias == "mysql" })
        let search = try #require(items.first { $0.alias == "search" })
        let postgres = try #require(items.first { $0.alias == "postgres" })
        #expect(
            PlanItemRow.line(mysql, displayName: name)
                == "mysql → erp-development-mysql · official/mysql 8.4.6 · 127.0.0.1:3307 · locked")
        #expect(PlanItemRow.line(search, displayName: name).hasSuffix("locked") == false)

        let overrides = EnvironmentPlanContent.appliedOverrides(plan)
        #expect(overrides.map(\.service) == ["mysql"])
        #expect(
            PlanItemRow.notes(mysql, displayName: name, overrides: overrides).contains(
                "Local override: port 3307"))
        #expect(
            PlanItemRow.notes(postgres, displayName: name, overrides: overrides)
                == ["Differs from lock: 17.7"])
        #expect(
            EnvironmentCopy.overrideNote(FixtureEnvironments.storageOverride)
                == "Local override: data in /Volumes/Fast/Stackly")

        #expect(EnvironmentPlanContent.staleServices(plan) == ["search"])
        #expect(
            EnvironmentCopy.staleLock(["search"])
                == "The lock is out of date for search. Those versions come from the catalog.")
        let local = try #require(plan.local)
        let warnings = try #require(EnvironmentPlanContent.localWarnings(local))
        #expect(warnings.map(\.name) == ["legacy"])
        #expect(
            EnvironmentCopy.localWarnings(local.file, count: warnings.count)
                == ".stackly.local.yaml has 1 warning.")
    }

    @Test func aLockedPlanWithABrokenLocalFileShowsBlockersAndNoOverride() throws {
        let plan = FixtureEnvironments.lockedBlockedPlan()
        #expect(EnvironmentPlanContent.staleServices(plan) == nil)
        #expect(EnvironmentPlanContent.appliedOverrides(plan).isEmpty)
        let items = EnvironmentPlanContent.items(plan.blockers ?? [])
        #expect(items.map(\.name) == ["search", "Environment"])
        #expect(
            items.first?.reason
                == "Service search is not in the lock. Run stackly env lock to update the lock.")
        #expect(EnvironmentPlanContent.localWarnings(try #require(plan.local)) == nil)
        #expect(plan.local?.overrides?.allSatisfy { $0.state == .ignored } == true)
    }

    @Test func anIgnoredOverrideIsNeverShownAsApplied() {
        let plan = FixtureEnvironments.lockedPlan()
        let overrides = EnvironmentPlanContent.appliedOverrides(plan)
        #expect(overrides.map(\.service) == ["mysql"])
        #expect(overrides.contains { $0.service == "legacy" } == false)
        #expect(overrides.contains { $0.state == .compared } == false)
    }
}
