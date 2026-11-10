import Foundation
import StacklyTestSupport
import Testing

@testable import StacklyUI

/// The navigator that sends the person from one page to another (spec 0022, *Navigasi*).
struct ShellNavigatorTests {
    @Test func showingADestinationChoosesItsSection() {
        let navigator = ShellNavigator()
        let destinations: [(ShellDestination, SidebarSection)] = [
            (.overview, .overview), (.instances(), .instances), (.catalog, .catalog),
            (.runtimes, .runtimes), (.operations, .operations), (.diagnostics, .diagnostics),
        ]
        for (destination, section) in destinations {
            navigator.show(destination)
            #expect(navigator.section == section)
        }
    }

    @Test func theInstancesDestinationLeavesARequestWithFilterAndSelection() {
        let navigator = ShellNavigator()
        let id = UUID()
        navigator.show(.instances(filter: .failed, select: id))
        #expect(navigator.instancesRequest?.filter == .failed)
        #expect(navigator.instancesRequest?.select == id)
    }

    @Test func aRequestStaysWhileTheInstancesSectionShows() {
        let navigator = ShellNavigator()
        navigator.show(.instances(filter: .failed))
        navigator.openInstance(UUID())
        let filter = navigator.instancesRequest
        let detail = navigator.instanceRequest
        // `AppShell` writes the section back from its scene storage.
        navigator.section = .instances
        navigator.showInstancesTable()
        #expect(navigator.instancesRequest == filter)
        #expect(navigator.instanceRequest == detail)
    }

    @Test func showingAnotherSectionDropsTheRequests() {
        let navigator = ShellNavigator()
        navigator.show(.instances(filter: .failed))
        navigator.openInstance(UUID())
        navigator.section = .overview
        #expect(navigator.instancesRequest == nil)
        #expect(navigator.instanceRequest == nil)

        navigator.show(.instances(filter: .stopped))
        navigator.show(.overview)
        #expect(navigator.instancesRequest == nil)
    }

    @Test func aSecondIdenticalRequestIsStillANewRequest() {
        let navigator = ShellNavigator()
        navigator.show(.instances(filter: .running))
        let first = navigator.instancesRequest
        navigator.show(.instances(filter: .running))
        #expect(navigator.instancesRequest != first)
    }

    @Test func otherDestinationsLeaveNoRequest() {
        let navigator = ShellNavigator()
        navigator.show(.catalog)
        #expect(navigator.instancesRequest == nil)
    }

    @Test func openingAnInstanceLeavesARequestForTheDetailPage() {
        let navigator = ShellNavigator()
        let id = UUID()
        navigator.openInstance(id)
        #expect(navigator.section == .instances)
        #expect(navigator.instanceRequest?.instance == id)
        #expect(navigator.instancesRequest == nil)
    }

    @Test func theInstanceDestinationShowsTheInstancesSection() {
        let navigator = ShellNavigator(section: .catalog)
        navigator.show(.instance(UUID()))
        #expect(navigator.section == .instances)
        #expect(ShellDestination.instance(UUID()).section == .instances)
    }

    @Test func aSecondRequestForTheSameInstanceIsANewRequest() {
        let navigator = ShellNavigator()
        let id = UUID()
        navigator.openInstance(id)
        let first = navigator.instanceRequest
        navigator.openInstance(id)
        #expect(navigator.instanceRequest != first)
    }

    @Test func otherDestinationsLeaveNoInstanceRequest() {
        let navigator = ShellNavigator()
        navigator.show(.instances(select: UUID()))
        navigator.show(.catalog)
        #expect(navigator.instanceRequest == nil)
    }

    @Test func askingForTheTableAgainCountsUp() {
        let navigator = ShellNavigator()
        let before = navigator.instancesResets
        navigator.showInstancesTable()
        #expect(navigator.instancesResets == before + 1)
    }

    // MARK: - Operations (spec 0035, AC-13)

    @Test func openingAnOperationLeavesANewRequestForTheOperationsPage() {
        let navigator = ShellNavigator(section: .instances)
        let id = UUID()
        navigator.openOperation(id)
        #expect(navigator.section == .operations)
        #expect(navigator.operationRequest?.operation == id)
        let first = navigator.operationRequest
        navigator.openOperation(id)
        #expect(navigator.operationRequest?.operation == id)
        #expect(navigator.operationRequest != first)
    }

    @Test func anOperationRequestStaysWhileOperationsShowsAndGoesWithAnotherSection() {
        let navigator = ShellNavigator()
        navigator.openOperation(UUID())
        let request = navigator.operationRequest
        // `AppShell` writes the section back from its scene storage.
        navigator.section = .operations
        #expect(navigator.operationRequest == request)
        navigator.show(.diagnostics)
        #expect(navigator.operationRequest == nil)
        #expect(ShellDestination.operations.section == .operations)
    }

    @Test func showingOperationsLeavesNoOperationRequest() {
        let navigator = ShellNavigator()
        navigator.show(.operations)
        #expect(navigator.section == .operations)
        #expect(navigator.operationRequest == nil)
        #expect(navigator.instancesRequest == nil)
    }
    @Test(
        .evidence(
            covers: ["REQ-019", "AC-019"],
            expected:
                "openDiagnostics shows Diagnostics with a new request that holds the focus, kept while Diagnostics shows and dropped with another section"
        ))
    func openDiagnosticsCarriesTheFocus() {
        let navigator = ShellNavigator()
        let id = UUID()
        navigator.openDiagnostics(focus: .instance(id))
        #expect(navigator.section == .diagnostics)
        #expect(navigator.diagnosticsRequest?.focus == .instance(id))
        let first = navigator.diagnosticsRequest
        navigator.section = .diagnostics
        #expect(navigator.diagnosticsRequest == first)
        navigator.openDiagnostics(focus: .instance(id))
        #expect(navigator.diagnosticsRequest != first)
        navigator.show(.overview)
        #expect(navigator.diagnosticsRequest == nil)
        navigator.openDiagnostics(focus: nil)
        #expect(navigator.diagnosticsRequest?.focus == nil)
        #expect(navigator.section == .diagnostics)
    }

    @Test(
        .evidence(
            covers: ["REQ-019"],
            expected:
                "Use Port of a finding opens the instance on Configuration with the endpoints sheet and the suggested port filled in"
        ))
    func openInstanceCanAskForTheEndpointsSheet() {
        let navigator = ShellNavigator()
        let id = UUID()
        let edit = InstanceEdit.editPorts(
            InstanceEditSource(id: id, name: "pg-main"), prefill: ["db": 5433])
        navigator.openInstance(id, tab: .configuration, edit: edit)
        #expect(navigator.section == .instances)
        #expect(navigator.instanceRequest?.instance == id)
        #expect(navigator.instanceRequest?.tab == .configuration)
        #expect(navigator.instanceRequest?.edit == edit)
        navigator.openInstance(id)
        #expect(navigator.instanceRequest?.edit == nil)
    }
}
