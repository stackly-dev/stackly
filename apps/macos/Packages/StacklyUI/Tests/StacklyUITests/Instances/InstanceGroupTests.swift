import StacklyClient
import Testing

@testable import StacklyUI

/// The status groups, the counts behind the Overview, and the filters of the Instances page
/// (spec 0022 AC-1, AC-2). The cases are the shared file the Rust rule reads too.
struct InstanceGroupTests {
    @Test(
        .evidence(
            covers: ["REQ-028", "NFR-012"],
            expected: "Every lifecycle times readiness case maps to the group of the shared file"))
    func everyCaseOfTheSharedFileMapsToItsGroup() throws {
        let cases = try InstanceContractFixture.groupCases()
        #expect(cases.count == 30)
        for entry in cases {
            let group = InstanceGroup.of(lifecycle: entry.lifecycle, readiness: entry.readiness)
            #expect(
                group?.rawValue == entry.group,
                "\(entry.lifecycle) with \(entry.readiness) gave \(String(describing: group))")
        }
    }

    @Test func aRunningInstanceThatIsNotReadyIsNeverCountedRunning() throws {
        let alive = try InstanceContractFixture.instance("analytics-db")
        #expect(alive.process != nil)
        #expect(InstanceGroup.of(alive) == .notReady)
        #expect(InstanceFilter.running.includes(.notReady))
    }

    @Test func onlyARetainedInstanceHasNoGroup() {
        for lifecycle in [InstanceLifecycle.retained] {
            for readiness in [InstanceReadinessState.unknown, .notReady, .ready] {
                #expect(InstanceGroup.of(lifecycle: lifecycle, readiness: readiness) == nil)
            }
        }
        #expect(InstanceGroup.of(lifecycle: .stopping, readiness: .ready) == .notReady)
    }

    @Test func theCountsOfTheMixedListAddUpToTheTotal() throws {
        let counts = InstanceCounts(of: try InstanceContractFixture.mixed())
        #expect(counts.total == 20)
        #expect(counts.ready == 4)
        #expect(counts.notReady == 8)
        #expect(counts.stopped == 4)
        #expect(counts.failed == 3)
        #expect(counts.unknown == 1)
        #expect(
            counts.ready + counts.notReady + counts.stopped + counts.failed + counts.unknown
                == counts.total)
    }

    @Test func eachFilterCountsTheRowsItLetsThrough() throws {
        let instances = try InstanceContractFixture.mixed()
        let counts = InstanceCounts(of: instances)
        for filter in InstanceFilter.allCases {
            let rows = instances.filter { filter.includes(InstanceGroup.of($0)) }
            #expect(rows.count == counts.count(in: filter), "\(filter)")
        }
        #expect(counts.count(in: .running) == counts.ready + counts.notReady)
        #expect(counts.count(in: .all) == counts.total)
    }

    @Test func anUnknownInstanceShowsOnlyUnderAll() throws {
        for filter in [InstanceFilter.running, .stopped, .failed] {
            #expect(filter.includes(.unknown) == false)
        }
        #expect(InstanceFilter.all.includes(.unknown))
        #expect(InstanceFilter.all.includes(nil) == false)
    }

    @Test func aRetainedInstanceCountsNowhere() throws {
        let retained = InstanceSummary.fixture(lifecycle: .retained)
        let counts = InstanceCounts(of: [retained])
        #expect(counts == InstanceCounts())
    }
}
