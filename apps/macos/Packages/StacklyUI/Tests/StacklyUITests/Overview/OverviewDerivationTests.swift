import Foundation
import StacklyClient
import Testing

@testable import StacklyUI

/// What the Overview derives from the instances and when it derives it again (spec 0022 AC-2,
/// AC-5, AC-14).
@MainActor
struct OverviewDerivationTests {
    private let instances: [InstanceSummary]

    init() throws {
        instances = try InstanceContractFixture.mixed().sorted {
            ($0.name, $0.id.uuidString) < ($1.name, $1.id.uuidString)
        }
    }

    // covers: AC-2
    @Test func theCountsOfTheDerivationAreTheGroupsOfTheMixedFixture() {
        let value = OverviewDerivation().value(summaries: instances, providers: nil, failures: [])
        #expect(value.counts.total == 20)
        #expect(value.counts.ready == 4)
        #expect(value.counts.notReady == 8)
        #expect(value.counts.stopped == 4)
        #expect(value.counts.failed == 3)
        #expect(value.counts.unknown == 1)
    }

    // covers: AC-5
    @Test func theRowsKeepTheOrderOfTheNamesSoTheSectionTakesTheFirstSixByName() {
        let value = OverviewDerivation().value(summaries: instances, providers: nil, failures: [])
        #expect(value.rows.map(\.name) == instances.map(\.name))
        let shown = value.rows.prefix(OverviewCopy.instanceLimit).map(\.name)
        #expect(shown == instances.prefix(6).map(\.name))
        #expect(shown == shown.sorted())
        #expect(value.rows.count > OverviewCopy.instanceLimit)
    }

    // covers: AC-5
    @Test func aFailedRefetchMarksOnlyItsOwnRowOfTheOverview() throws {
        let sick = try #require(instances.first { $0.name == "sick-db" })
        let value = OverviewDerivation().value(
            summaries: instances, providers: nil, failures: [sick.id])
        #expect(value.rows.filter(\.refreshFailed).map(\.id) == [sick.id])
    }

    // covers: AC-14
    @Test func theSameInputGivesTheHeldValueWithoutDerivingAgain() {
        let derivation = OverviewDerivation()
        _ = derivation.value(summaries: instances, providers: nil, failures: [])
        _ = derivation.value(summaries: instances, providers: nil, failures: [])
        _ = derivation.value(summaries: instances, providers: nil, failures: [])
        #expect(derivation.recomputes == 1)
    }

    // covers: AC-14
    @Test func aChangedInstanceTheCatalogOrTheFailedRefetchesDerivesAgain() {
        let derivation = OverviewDerivation()
        _ = derivation.value(summaries: instances, providers: nil, failures: [])
        #expect(derivation.recomputes == 1)

        let changed = instances + [.fixture(number: 200, name: "zz-new", port: 25_000)]
        let afterEvent = derivation.value(summaries: changed, providers: nil, failures: [])
        #expect(derivation.recomputes == 2)
        #expect(afterEvent.counts.stopped == InstanceCounts(of: changed).stopped)
        #expect(afterEvent.rows.count == instances.count + 1)

        let entry = ProviderSummary.fixture(
            provider: "official/postgresql", displayName: "PostgreSQL", mark: "Pg", tile: .blue)
        _ = derivation.value(summaries: changed, providers: [entry], failures: [])
        #expect(derivation.recomputes == 3)

        _ = derivation.value(summaries: changed, providers: [entry], failures: [changed[0].id])
        #expect(derivation.recomputes == 4)

        _ = derivation.value(summaries: changed, providers: [entry], failures: [changed[0].id])
        #expect(derivation.recomputes == 4)
    }
}
