import Foundation
import StacklyClient
import Testing

/// The shared instance data in `protocols/fixtures/instance/`, the files the Rust tests of
/// `stackly instance list` read too (spec 0022).
enum InstanceContractFixture {
    /// One case of `groups.json`: a lifecycle, a readiness state, and the group the shared
    /// rule gives, or `nil` for a retained instance.
    struct GroupCase: Decodable {
        let lifecycle: InstanceLifecycle
        let readiness: InstanceReadinessState
        let group: String?
    }

    private struct GroupCases: Decodable {
        let cases: [GroupCase]
    }

    /// Every case of `groups.json`.
    static func groupCases() throws -> [GroupCase] {
        try decode(GroupCases.self, "groups").cases
    }

    /// The instances of `list-mixed.json`, in the order of the file.
    static func mixed() throws -> [InstanceSummary] {
        try decode(InstancePage.self, "list-mixed").items ?? []
    }

    /// The instance named `name` in `list-mixed.json`.
    static func instance(_ name: String) throws -> InstanceSummary {
        try #require(try mixed().first { $0.name == name }, "no instance named \(name)")
    }

    /// The operations of `attention-operations.json`, in the order of the file.
    static func operations() throws -> [OperationSummary] {
        try decode(OperationPage.self, "attention-operations").items ?? []
    }

    private static func decode<Value: Decodable>(_ type: Value.Type, _ name: String) throws -> Value
    {
        let url = SourceTree.url(of: "protocols/fixtures/instance/\(name).json")
        return try StacklyJSON.decoder().decode(Value.self, from: Data(contentsOf: url))
    }

    /// A report as the daemon sends it.
    static func capacity(
        _ level: CapacityLevel, total: UInt64 = 1_000_000_000_000,
        available: UInt64 = 500_000_000_000, checkedAt: Date = Date(timeIntervalSince1970: 1000)
    ) -> CapacityReport {
        CapacityReport(
            checkedAt: checkedAt, totalBytes: total, availableBytes: available,
            usedPercent: UInt8((total - available) * 100 / total), level: level)
    }
}
