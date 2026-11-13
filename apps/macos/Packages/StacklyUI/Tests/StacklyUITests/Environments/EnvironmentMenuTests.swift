import Foundation
import StacklyClient
import Testing

@testable import StacklyUI

/// The menu of a service row and the More menu of an environment (spec 0044, AC-19, AC-21).
@MainActor
struct EnvironmentMenuTests {
    private func rows(_ rig: PageRig) throws -> [ServiceRowModel] {
        let detail = try #require(FixtureEnvironments.detail(FixtureEnvironments.erpID))
        return ServiceRowModel.rows(of: detail, instances: rig.instances.store.items)
    }

    @Test(
        .evidence(
            covers: ["NFR-012"],
            expected:
                "A service with an instance offers Open In, Copy Connection URL, Show Instance, Show Logs, and its lifecycle action; one not created offers only Show Plan…"
        ))
    func aServiceMenuFollowsItsInstance() async throws {
        let rig = PageRig(
            instances: FixtureEnvironments.instances, environments: FixtureEnvironments.summaries)
        await rig.load()
        let rows = try rows(rig)
        var opened: [UUID] = []
        var logs: [UUID] = []
        var plans = 0
        let openIn = OpenInContent.sections([
            [
                StacklyAction(
                    id: "openIn.terminal", title: "Terminal", mutates: false, perform: {})
            ]
        ])
        let copy = ConnectionCopier.action(
            detail: nil, secrets: rig.detail.secrets, report: { _ in })
        let context = InstanceActionContext.make(feedbackCenter: nil, navigator: nil)
        func menu(_ alias: String) throws -> [[StacklyAction]] {
            let row = try #require(rows.first { $0.alias == alias })
            return ServiceMenu.sections(
                for: row, openIn: openIn, copy: copy,
                lifecycle: row.summary.map { rig.actions.stacklyAction(for: $0, context: context) },
                open: { opened.append($0) }, openLogs: { logs.append($0) },
                showPlan: { plans += 1 })
        }

        let postgres = try menu("postgres")
        #expect(
            postgres.map { $0.map(\.title) } == [
                ["Terminal"], ["Copy Connection URL"], ["Show Instance", "Show Logs"], ["Stop"],
            ])
        postgres[2][0].perform()
        postgres[2][1].perform()
        let id = FixtureEnvironments.instances[0].id
        #expect(opened == [id])
        #expect(logs == [id])
        #expect(try menu("queue").last?.map(\.title) == ["Retry"])

        let search = try menu("search")
        #expect(search.map { $0.map(\.title) } == [["Show Plan…"]])
        search[0][0].perform()
        #expect(plans == 1)
        let row = try #require(rows.first { $0.alias == "search" })
        #expect(
            ServiceMenu.sections(
                for: row, openIn: .hidden, copy: nil, lifecycle: nil, open: { _ in },
                openLogs: { _ in }, showPlan: nil
            ).isEmpty)
    }

    @Test func theMoreMenuOpensRevealsAndReloadsTheManifest() throws {
        var opened: [URL] = []
        var revealed: [URL] = []
        var reloads = 0
        let ports = ManifestFilePorts(
            open: { opened.append($0) }, reveal: { revealed.append($0) })
        let remove = StacklyAction(
            id: "environment.remove", title: "Remove Environment…", mutates: true,
            perform: {})
        let path = FixtureEnvironments.manifest("erp")
        let sections = EnvironmentFileActions.sections(
            location: path, ports: ports, reload: { reloads += 1 }, remove: remove,
            isReloading: false)
        #expect(
            sections.map { $0.map(\.title) } == [
                ["Open Manifest", "Reveal in Finder", "Copy Path", "Reload Manifest"],
                ["Remove Environment…"],
            ])
        let file = try #require(sections.first)
        file[0].perform()
        file[1].perform()
        file[3].perform()
        #expect(opened == [URL(fileURLWithPath: path)])
        #expect(revealed == [URL(fileURLWithPath: path)])
        #expect(reloads == 1)
        #expect(file.allSatisfy { $0.disabledReason == nil })
    }

    @Test func withoutAFileTheFileActionsSayWhy() {
        let sections = EnvironmentFileActions.sections(
            location: nil, ports: ManifestFilePorts(open: { _ in }, reveal: { _ in }),
            reload: nil, remove: nil, isReloading: false)
        #expect(sections.count == 1)
        #expect(
            sections[0].allSatisfy {
                $0.disabledReason == "This environment has no manifest file."
            })
    }
}
