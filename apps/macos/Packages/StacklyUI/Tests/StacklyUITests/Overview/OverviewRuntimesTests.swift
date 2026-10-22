import Foundation
import StacklyClient
import Testing

@testable import StacklyUI

/// The Runtimes card and the titles of the install operations (spec 0022 AC-2, AC-5): the
/// card follows the `RuntimeStore`, and an install is named by its `installing` row.
@MainActor
struct OverviewRuntimesTests {
    private func rig(
        runtimes: RuntimeListPage? = FixtureRuntime.page, load: Bool = true
    ) async -> PageRig {
        let rig = PageRig(runtimes: runtimes)
        if load { await rig.load() }
        return rig
    }

    // MARK: - The card

    @Test func noRuntimeStoreDrawsNoCard() async {
        let rig = await rig(runtimes: nil)
        #expect(RuntimesCardState.of(rig.runtimes) == nil)
    }

    @Test func theCardShowsNoNumberBeforeTheFirstSnapshot() async {
        let rig = await rig(load: false)
        let state = RuntimesCardState.of(rig.runtimes)
        #expect(state == .loading)
        #expect(state?.count == nil)
        #expect(state?.note == nil)
    }

    @Test func theCardCountsTheRuntimesOfThePageSubtitle() async throws {
        let rig = await rig()
        let runtimes = try #require(rig.runtimes)
        #expect(RuntimesCardState.of(runtimes) == .loaded(FixtureRuntime.summaries.count))
        #expect(runtimes.subtitle == "\(FixtureRuntime.summaries.count) installed")
    }

    @Test func aStoreWithNoRuntimeCountsZeroOnceLoaded() async {
        let rig = await rig(runtimes: RuntimeListPage(items: [], next: nil, installing: []))
        #expect(RuntimesCardState.of(rig.runtimes) == .loaded(0))
    }

    @Test func aStoreThatFailedWithoutDataSaysItCouldNotLoad() async {
        let rig = await rig(load: false)
        rig.script.runtimeFailure = .rejected(PageRig.failure)
        await rig.load()
        let state = RuntimesCardState.of(rig.runtimes)
        #expect(state == .failed)
        #expect(state?.note == "Couldn’t load")
        #expect(state?.count == nil)
    }

    @Test func aFailedRefreshKeepsTheOldNumber() async {
        let rig = await rig()
        rig.script.runtimeFailure = .rejected(PageRig.failure)
        await rig.runtimes?.store.refresh()
        #expect(RuntimesCardState.of(rig.runtimes) == .loaded(FixtureRuntime.summaries.count))
    }

    @Test func aDaemonWithoutRuntimeListDrawsNoCard() async {
        let rig = await rig(load: false)
        rig.script.runtimeFailure = .rejected(PageRig.unknownMethod)
        await rig.load()
        #expect(rig.runtimes?.store.unsupported == true)
        #expect(RuntimesCardState.of(rig.runtimes) == nil)
    }

    @Test func theCardIsSpokenWithItsNumberOrItsFailure() {
        #expect(SummaryCard.spokenLabel(label: "Runtimes", count: 4, note: nil) == "Runtimes, 4")
        #expect(SummaryCard.spokenLabel(label: "Runtimes", count: 0, note: nil) == "Runtimes, 0")
        #expect(
            SummaryCard.spokenLabel(label: "Runtimes", count: nil, note: nil)
                == "Runtimes, loading")
        #expect(
            SummaryCard.spokenLabel(label: "Runtimes", count: nil, note: "Couldn’t load")
                == "Runtimes, Couldn’t load")
    }

    // MARK: - The titles

    @Test func anInstallIsNamedByTheInstallingRowOfItsOperation() async {
        let rig = await rig()
        let names = OperationTitle.RuntimeNames(store: rig.runtimes)
        let install = FixtureScenario.operationCases[0]
        #expect(install.kind == .installRuntime)
        #expect(OverviewTitle.of(install, names) == "Install PostgreSQL 18.1")
        #expect(
            names.installed[FixtureRuntime.summaries[0].id] == "PostgreSQL 17.6")
    }

    @Test func anInstallWithoutAnInstallingRowIsJustARuntime() async {
        let rig = await rig(runtimes: RuntimeListPage(items: [], next: nil, installing: []))
        let names = OperationTitle.RuntimeNames(store: rig.runtimes)
        #expect(OverviewTitle.of(FixtureScenario.operationCases[0], names) == "Install runtime")
        #expect(
            OverviewTitle.of(
                FixtureScenario.operationCases[0], OperationTitle.RuntimeNames(store: nil))
                == "Install runtime")
    }

    @Test func theRowOfAnInstallReadsItsTitleAndItsPercent() {
        let install = OperationSnapshot.fixture(
            number: 1, kind: .installRuntime, state: .running, stage: "download",
            progress: .counted(CountedProgress(done: 42, total: 100)))
        #expect(
            OverviewOperationRow.spokenLabel(operation: install, title: "Install PostgreSQL 18.1")
                == "Install PostgreSQL 18.1, Running, Download, 8 percent")
    }

    @Test func aLaterStageCountsTheStagesBeforeIt() {
        let install = OperationSnapshot.fixture(
            number: 1, kind: .installRuntime, state: .running, stage: "extract",
            progress: .counted(CountedProgress(done: 50, total: 100)))
        #expect(
            OverviewOperationRow.spokenLabel(operation: install, title: "Install Valkey 8.1.4")
                == "Install Valkey 8.1.4, Running, Extract, 50 percent")
    }

    @Test func anInstallThatStartedElsewhereGetsItsNameWhenTheStoreReadsAgain() async throws {
        let rig = await rig(runtimes: RuntimeListPage(items: [], next: nil, installing: []))
        let runtimes = try #require(rig.runtimes)
        #expect(OperationTitle.RuntimeNames(store: runtimes).installing.isEmpty)
        rig.script.runtimePage = FixtureRuntime.page
        await runtimes.reloadInstalling()
        #expect(
            OperationTitle.RuntimeNames(store: runtimes).installing[
                FixtureRuntime.installing.operationId]
                == "PostgreSQL 18.1")
    }
}

/// The title of an operation with only runtime names, as the Overview computes it.
private enum OverviewTitle {
    static func of(_ operation: OperationSnapshot, _ names: OperationTitle.RuntimeNames) -> String {
        OperationTitle.title(for: operation, instanceNames: [:], runtimes: names)
    }
}
