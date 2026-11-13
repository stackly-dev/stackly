import Foundation
import StacklyClient
import StacklyTestSupport
import Testing

@testable import StacklyUI

/// The plan sheet reads the state of the local override file (spec 0045 K22, AC-19): a file
/// that is not `present` applies no override, even from a daemon that still marks one
/// `applied` and sends no `env.local_invalid` blocker.
@MainActor
struct EnvironmentLocalStateTests {
    private func plan(local state: LocalState, marking mark: OverrideState) -> EnvPlan {
        let locked = FixtureEnvironments.lockedPlan()
        let base = locked.local
        let overrides = (base?.overrides ?? []).map {
            PlanOverride(service: $0.service, field: $0.field, value: $0.value, state: mark)
        }
        return EnvPlan(
            validation: locked.validation, registration: locked.registration,
            environment: locked.environment, registeredLocation: locked.registeredLocation,
            applicable: locked.applicable, planDigest: locked.planDigest, items: locked.items,
            order: locked.order, blockers: [], detached: locked.detached,
            needsSecrets: locked.needsSecrets, lock: locked.lock,
            local: PlanLocal(
                file: base?.file ?? ".stackly.local.yaml", state: state, sha256: base?.sha256,
                overrides: overrides, findings: base?.findings))
    }

    @Test func aFileThatIsNotPresentAppliesNoOverrideEvenWithoutItsBlocker() throws {
        for state in [LocalState.invalid, .unreadable] {
            let plan = plan(local: state, marking: .applied)
            #expect(EnvironmentPlanContent.appliedOverrides(plan).isEmpty, "\(state)")
            let items = try #require(plan.items)
            let mysql = try #require(items.first { $0.alias == "mysql" })
            let notes = PlanItemRow.notes(
                mysql, displayName: { $0 },
                overrides: EnvironmentPlanContent.appliedOverrides(plan))
            #expect(notes.contains { $0.hasPrefix("Local override:") } == false, "\(state)")
        }
    }

    @Test func aPresentFileShowsOnlyItsAppliedOverrides() {
        #expect(
            EnvironmentPlanContent.appliedOverrides(plan(local: .present, marking: .applied))
                .isEmpty == false)
        for mark in [OverrideState.ignored, .compared, .notAppliedExisting] {
            #expect(
                EnvironmentPlanContent.appliedOverrides(plan(local: .present, marking: mark))
                    .isEmpty, "\(mark)")
        }
    }
}
