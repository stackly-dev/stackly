import Foundation
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
        navigator.consumeInstancesRequest()
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
        navigator.consumeInstanceRequest()
        #expect(navigator.instanceRequest == nil)
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
}
