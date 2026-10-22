import Foundation
import StacklyClient
import Testing

/// What the runtime store of the app needs from the generated types (spec 0018 AC-17,
/// AC-18): the four runtime calls name their methods, and a runtime and a running install
/// are known by their IDs.
struct RuntimeIdentityTests {
    @Test func theRuntimeCallsNameTheirMethods() {
        let params = InstallRuntimeParams(
            provider: "official/nats", selector: "2.11.9", expectedSha256: nil)
        #expect(Call.runtimeInstall(params).methodName == "runtime.install")
        #expect(Call.runtimeResolve(params).methodName == "runtime.resolve")
        #expect(
            Call.runtimeList(ListRuntimesParams(limit: nil, after: nil)).methodName
                == "runtime.list")
        #expect(
            Call.runtimeGet(GetRuntimeParams(runtimeId: UUID())).methodName == "runtime.get")
    }

    @Test func aRuntimeSummaryIsKnownByItsIdAndARunningInstallByItsOperation() throws {
        let list = try RuntimeContractTests.runtime(RuntimeListPage.self, "list.json")
        let summary = try #require(list.items?.first)
        #expect(summary.id == summary.id as UUID)
        let install = try #require(list.installing?.first)
        #expect(install.id == install.operationId)
        #expect(install.id != install.runtimeId)
    }
}
