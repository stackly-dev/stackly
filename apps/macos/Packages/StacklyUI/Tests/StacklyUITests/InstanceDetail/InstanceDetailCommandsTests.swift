import Foundation
import StacklyClient
import Testing

@testable import StacklyUI

/// What the Instances page publishes for the detail page (spec 0021 AC-11, AC-12, AC-15; spec
/// 0069 AC-11 and AC-12), and the models the page keeps for its instances.
@MainActor
struct InstanceDetailCommandsTests {
    private let item: DetailFixture.Triple

    init() throws {
        item = try DetailFixture.triple("postgresql")
    }

    private func action(_ id: String, title: String = "x", reason: String? = nil)
        -> StacklyAction
    {
        StacklyAction(id: id, title: title, mutates: false, disabledReason: reason, perform: {})
    }

    private func publish(
        open: StacklyAction? = nil, back: (@MainActor () -> Void)? = nil,
        copy: StacklyAction? = nil
    ) -> [CommandID: StacklyAction] {
        InstancesCommandActions.publish(
            showsInspector: true, toggleInspector: {}, sort: .default, setSort: { _ in },
            target: nil, open: open, back: back, copyConnection: copy
        ).actions
    }

    @Test func openBackAndCopyArePublishedOnlyWhenTheyHaveATarget() {
        let none = publish()
        #expect(none[.openInstance] == nil)
        #expect(none[.back] == nil)
        #expect(none[.copyConnectionString] == nil)

        let all = publish(
            open: action("anything"), back: {}, copy: action("whatever", reason: "why"))
        #expect(all[.openInstance]?.id == "openInstance")
        #expect(all[.back]?.title == "Back")
        #expect(all[.copyConnectionString]?.id == "copyConnectionString")
        #expect(all[.copyConnectionString]?.disabledReason == "why")
    }

    @Test func backRunsTheClosureOfThePage() {
        var went = false
        let actions = publish(back: { went = true })
        actions[.back]?.perform()
        #expect(went)
    }

    @Test func everyPublishedCommandHasABuiltRowOnTheInstancesPage() {
        let all = publish(open: action("a"), back: {}, copy: action("b"))
        for id in all.keys {
            let entry = CommandMap.entry(for: id)
            #expect(entry.status == .built, "\(id)")
            #expect(entry.scope.includesInstances, "\(id)")
        }
    }

    // MARK: - Models

    @Test func theRegistryKeepsOneModelPerInstanceAndDropsTheGoneOnes() {
        let script = DetailScript(detail: item.detail)
        let models = InstanceDetailModels(service: script.service)
        let first = models.model(for: item.detail.instanceId)
        #expect(models.model(for: item.detail.instanceId) === first)
        let other = models.model(for: UUID())
        #expect(other !== first)

        models.keep([item.detail.instanceId])
        #expect(models.model(for: item.detail.instanceId) === first)
        #expect(models.model(for: other.instanceId) !== other)
    }

    @Test func aWindowWithoutADaemonFailsEveryReadAsNotConnected() async {
        let model = InstanceDetailModels(service: .disconnected).model(for: UUID())
        await model.open()
        #expect(model.detail == nil)
        #expect(model.detailError?.code == "ipc.not_connected")
        #expect(model.dataSizeState == .unavailable)
    }

    @Test func theInspectorReadsTheSizeFromTheCacheOnly() async {
        let script = DetailScript(detail: item.detail, usage: item.dataUsage)
        let model = InstanceDetailModels(service: script.service).model(for: item.detail.instanceId)
        await model.openForInspector()

        #expect(script.usageCalls.count == 1)
        #expect(script.usageCalls.first?.cachedOnly == true)
        #expect(script.usageCalls.first?.refresh == false)
        #expect(model.detail == item.detail)
    }

}

extension CommandScope {
    /// Whether the scope covers the Instances page.
    fileprivate var includesInstances: Bool {
        switch self {
        case .app, .shell: false
        case .page(let sections): sections.contains(.instances)
        }
    }
}
