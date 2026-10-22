import Foundation
import StacklyClient
import Testing

@testable import StacklyUI

/// The shared detail data in `protocols/fixtures/instance/` (spec 0021 AC-7): the triples the
/// Rust golden tests of `stackly instance inspect` read too, and the compose cases of
/// `connection-compose.json`.
enum DetailFixture {
    /// One `detail-*.json` file: the three values `stackly instance inspect --json` writes as
    /// its `result`.
    struct Triple: Decodable {
        let instance: InstanceSummary
        let detail: InstanceDetail
        let dataUsage: DataUsage?
    }

    /// One case of `connection-compose.json`.
    struct ComposeCase: Decodable {
        let name: String
        let parts: [ConnectionPart]
        let values: [String: String]
        let expected: String?
        let missingValue: String?
    }

    private struct ComposeFile: Decodable {
        let compose: [ComposeCase]
    }

    /// The names of the four triples.
    static let names = ["postgresql", "nats", "unavailable", "package-unreadable"]

    /// The triple `detail-<name>.json`.
    static func triple(_ name: String) throws -> Triple {
        try decode(Triple.self, "detail-\(name)")
    }

    /// Every compose case of `connection-compose.json`, in file order.
    static func composeCases() throws -> [ComposeCase] {
        try decode(ComposeFile.self, "connection-compose").compose
    }

    private static func decode<Value: Decodable>(_ type: Value.Type, _ name: String) throws -> Value
    {
        let url = SourceTree.url(of: "protocols/fixtures/instance/\(name).json")
        return try StacklyJSON.decoder().decode(Value.self, from: Data(contentsOf: url))
    }
}
