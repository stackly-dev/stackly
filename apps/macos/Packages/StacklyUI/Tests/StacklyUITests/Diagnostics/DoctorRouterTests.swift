import Foundation
import StacklyClient
import StacklyTestSupport
import Testing

@testable import StacklyUI

/// Where the safe action of a finding goes (spec 0034, AC-18): only places and flows that ask
/// first.
struct DoctorRouterTests {
    @Test(
        .evidence(
            covers: ["REQ-019", "AC-019"],
            expected:
                "Show Instance, Review, Use Port, and Show Operation only navigate; Use Port opens the endpoints sheet with the suggested port filled in"
        ))
    func routesOnlyNavigate() {
        let navigator = ShellNavigator()
        var opened = 0
        let router = DoctorRouter(
            navigator: navigator, settingsTab: SettingsTabSelection(),
            openSettings: { opened += 1 })
        let id = UUID()

        router.open(.showInstance(id), finding: nil)
        #expect(navigator.section == .instances)
        #expect(navigator.instanceRequest?.instance == id)
        #expect(navigator.instanceRequest?.tab == .summary)

        router.open(.reviewConfig(id), finding: nil)
        #expect(navigator.instanceRequest?.tab == .configuration)
        #expect(navigator.instanceRequest?.edit == nil)

        let finding = DoctorFixture.finding(instance: DoctorInstanceRef(id: id, name: "pg-main"))
        router.open(.changePort(instance: id, endpoint: "db", port: 5433), finding: finding)
        #expect(navigator.instanceRequest?.tab == .configuration)
        #expect(
            navigator.instanceRequest?.edit
                == .editPorts(InstanceEditSource(id: id, name: "pg-main"), prefill: ["db": 5433]))

        let operation = UUID()
        router.open(.showOperation(operation), finding: nil)
        #expect(navigator.section == .operations)
        #expect(navigator.operationRequest?.operation == operation)
        #expect(opened == 0)
    }

    @Test(
        .evidence(
            covers: ["REQ-019", "AC-019"],
            expected: "Open Settings selects the Storage tab, then opens the Settings window"
        ))
    func openSettingsShowsStorage() {
        let selection = SettingsTabSelection()
        var opened: [SettingsTab] = []
        let router = DoctorRouter(
            navigator: ShellNavigator(), settingsTab: selection,
            openSettings: { opened.append(selection.tab) })
        #expect(selection.tab == .general)
        router.open(.openStorageSettings, finding: nil)
        #expect(opened == [.storage])
    }
}
