import Foundation
import StacklyClient
import Testing

@testable import StacklyUI

/// The search, the sort, and the rows of the Instances table (spec 0022 AC-6, AC-9).
struct InstanceSearchSortTests {
    private func rows(catalog: [ProviderSummary]? = nil, failures: Set<UUID> = []) throws
        -> [InstanceRowModel]
    {
        InstanceRowModel.rows(
            of: try InstanceContractFixture.mixed(), catalog: catalog, failures: failures)
    }

    private func names(_ rows: [InstanceRowModel]) -> [String] { rows.map(\.name) }

    // MARK: - Rows

    @Test func theProviderIsTheSoftwareUntilTheCatalogLoaded() throws {
        let row = try #require(try rows().first { $0.name == "orders-db" })
        #expect(row.providerTitle == "postgresql")
        #expect(row.tile == TileSpec(mark: nil, tile: nil, name: "postgresql"))
    }

    @Test func theProviderComesFromTheCatalogEntryOfTheSameProvider() throws {
        let entry = ProviderSummary.fixture(
            provider: "official/postgresql", displayName: "PostgreSQL", mark: "Pg", tile: .blue)
        let row = try #require(try rows(catalog: [entry]).first { $0.name == "orders-db" })
        #expect(row.providerTitle == "PostgreSQL")
        #expect(row.tile == TileSpec(mark: "Pg", tile: .blue, name: "PostgreSQL"))
        #expect(row.providerAndVersion == "PostgreSQL 17.2.0")
    }

    @Test func aFailedRefetchReplacesTheDetailOfTheStatus() throws {
        let all = try rows()
        let id = try #require(all.first { $0.name == "sick-db" }?.id)
        let failed = try rows(failures: [id]).first { $0.id == id }
        #expect(failed?.appearance.detail == StatusDetail(text: "Refresh failed", tone: .caution))
        #expect(failed?.refreshFailed == true)
        let others = try rows(failures: [id]).filter { $0.id != id }
        #expect(others.contains { $0.appearance.detail?.text == "Refresh failed" } == false)
    }

    // MARK: - Search

    @Test func everyWordMustMatchOneField() throws {
        let all = try rows()
        func found(_ query: String) -> [String] {
            names(all.filter { InstanceSearch.matches($0, query: query) })
        }
        #expect(found("").count == all.count)
        #expect(found("orders") == ["orders-db"])
        #expect(found("orders 17.2") == ["orders-db"])
        #expect(found("postgresql 17.2").contains("orders-db"))
        #expect(found("postgresql 17.2").contains("cache") == false)
        #expect(found("5432 orders") == ["orders-db"])
        #expect(found("orders analytics") == [])
        #expect(found("ORDERS") == ["orders-db"])
    }

    @Test func aPortMatchesWithAndWithoutTheColon() throws {
        let all = try rows()
        let withColon = all.filter { InstanceSearch.matches($0, query: ":5432") }
        let without = all.filter { InstanceSearch.matches($0, query: "5432") }
        #expect(names(withColon) == names(without))
        #expect(names(without).contains("orders-db"))
    }

    @Test func searchIgnoresDiacriticsAndMatchesTheDisplayName() throws {
        let all = try rows()
        #expect(all.filter { InstanceSearch.matches($0, query: "ordérs") }.count == 1)
        #expect(
            names(all.filter { InstanceSearch.matches($0, query: "Orders database") }) == [
                "orders-db"
            ])
    }

    @Test func eachWordIsCheckedAgainstItsOwnField() throws {
        // “postgresql” is the software of analytics-db, and “analytics” its name; a row has
        // to match both words, each in a field of its own.
        let all = try rows()
        #expect(
            names(all.filter { InstanceSearch.matches($0, query: "analytics postgresql") }) == [
                "analytics-db"
            ])
        #expect(all.filter { InstanceSearch.matches($0, query: "cache postgresql") }.isEmpty)
    }

    // MARK: - Sort

    @Test func theDefaultOrderIsTheNameAscending() throws {
        let sorted = InstanceRowModel.sorted(try rows(), by: .default)
        #expect(
            names(sorted)
                == names(sorted).sorted { $0.localizedStandardCompare($1) == .orderedAscending })
    }

    @Test func reversingTheNameReversesTheWholeColumn() throws {
        let forward = names(InstanceRowModel.sorted(try rows(), by: InstanceSortComparator(.name)))
        let reverse = names(
            InstanceRowModel.sorted(try rows(), by: InstanceSortComparator(.name, order: .reverse)))
        #expect(reverse == forward.reversed())
    }

    @Test func aMissingValueIsLastInBothDirections() throws {
        let all = try rows()
        for order in [SortOrder.forward, .reverse] {
            let byPort = InstanceRowModel.sorted(
                all, by: InstanceSortComparator(.endpoints, order: order))
            let ports = byPort.map { InstanceText.firstPort($0.summary.endpoints) }
            let firstMissing = ports.firstIndex(of: nil)
            if let firstMissing {
                #expect(ports[firstMissing...].allSatisfy { $0 == nil }, "\(order)")
            }
        }
    }

    @Test func aTieFallsBackToTheNameThenTheIDAscendingInBothDirections() throws {
        let a = InstanceSummary.fixture(number: 1, name: "alpha", runtimeVersion: "17.6")
        let b = InstanceSummary.fixture(number: 2, name: "beta", runtimeVersion: "17.6")
        let rows = InstanceRowModel.rows(of: [b, a], catalog: nil)
        for order in [SortOrder.forward, .reverse] {
            let sorted = InstanceRowModel.sorted(
                rows, by: InstanceSortComparator(.version, order: order))
            #expect(names(sorted) == ["alpha", "beta"], "\(order)")
        }
    }

    @Test func versionsSortAsNumbers() {
        let low = InstanceSummary.fixture(number: 1, name: "a", runtimeVersion: "9.6")
        let high = InstanceSummary.fixture(number: 2, name: "b", runtimeVersion: "17.6")
        let rows = InstanceRowModel.rows(of: [high, low], catalog: nil)
        #expect(
            names(InstanceRowModel.sorted(rows, by: InstanceSortComparator(.version))) == [
                "a", "b",
            ])
    }

    @Test func statusSortsFailedUnknownNotReadyReadyStopped() throws {
        let sorted = InstanceRowModel.sorted(try rows(), by: InstanceSortComparator(.status))
        let ranks = sorted.compactMap { $0.group?.statusRank }
        #expect(ranks == ranks.sorted())
        #expect(sorted.first?.group == .failed)
        #expect(sorted.last?.group == .stopped)
    }

    @Test func providerSortsByTheTitleOfTheRow() throws {
        let sorted = InstanceRowModel.sorted(try rows(), by: InstanceSortComparator(.provider))
        let titles = sorted.map(\.providerTitle)
        #expect(titles == titles.sorted { $0.localizedStandardCompare($1) == .orderedAscending })
    }
}
