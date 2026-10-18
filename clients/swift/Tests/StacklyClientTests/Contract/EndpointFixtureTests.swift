import Darwin
import Foundation
import Testing

@testable import StacklyClient

/// The trust rules and the errno names match Rust through the shared fixture
/// `protocols/fixtures/endpoint/checks.json` (spec 0013 AC-2, AC-20).
struct EndpointFixtureTests {
    struct Facts: Decodable, Sendable {
        let type: String
        let uid: UInt32
        let mode: String

        var stat: FileStat {
            let kind: FileStat.Kind =
                switch type {
                case "socket": .socket
                case "dir": .directory
                case "symlink": .symlink
                default: .other
                }
            return FileStat(kind: kind, uid: uid, mode: mode_t(mode, radix: 8) ?? 0o777)
        }
    }

    struct Check: Decodable, Sendable, CustomTestStringConvertible {
        let name: String
        let path: Facts
        let parent: Facts
        let uid: UInt32
        let reason: String?

        var testDescription: String { name }
    }

    struct Errno: Decodable, Sendable {
        let errno: String
        let number: Int32
        let kind: String
    }

    struct Document: Decodable, Sendable {
        let checks: [Check]
        let errno: [Errno]
    }

    static func document() throws -> Document {
        try JSONDecoder().decode(Document.self, from: Fixture.data("endpoint/checks.json"))
    }

    @Test(arguments: try EndpointFixtureTests.document().checks)
    func checkFailsWithTheFixtureReason(_ check: Check) {
        let reason = EndpointCheck.evaluate(
            path: check.path.stat, parent: check.parent.stat, uid: check.uid)
        #expect(reason?.rawValue == check.reason)
    }

    @Test func everyFixtureErrnoHasTheRustKindName() throws {
        let table = try Self.document().errno
        for required in [
            "EACCES", "EPERM", "ENOENT", "ENAMETOOLONG", "ENOTDIR", "EMFILE", "ENFILE", "ELOOP",
        ] {
            #expect(table.contains { $0.errno == required }, "missing \(required)")
        }
        #expect(table.contains { $0.kind == ErrnoKind.fallback })
        for entry in table {
            #expect(ErrnoKind.name(of: entry.number) == entry.kind, "\(entry.errno)")
        }
        #expect(table.first { $0.errno == "EACCES" }?.number == EACCES)
        #expect(table.first { $0.errno == "ENAMETOOLONG" }?.number == ENAMETOOLONG)
    }

    @Test func untrustedReasonsAreTheFixedListOfSpec0006() {
        #expect(
            UntrustedReason.allCases.map(\.rawValue) == [
                "not_a_socket", "symlink", "wrong_owner", "group_or_other_access",
                "parent_wrong_owner", "parent_group_or_other_access", "peer_uid_mismatch",
                "peer_credentials_unavailable",
            ])
    }
}
