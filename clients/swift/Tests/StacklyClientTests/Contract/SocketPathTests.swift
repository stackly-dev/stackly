import Darwin
import Foundation
import Testing

@testable import StacklyClient

/// The socket path the Swift client derives matches the Rust layout byte for
/// byte, through the shared fixture `protocols/fixtures/layout/socket-paths.json`
/// (spec 0013 AC-1, AC-20).
struct SocketPathTests {
    struct Vector: Decodable, Sendable {
        let input: String
        let hex: String
    }

    struct Case: Decodable, Sendable, CustomTestStringConvertible {
        let name: String
        let tempDir: String
        let override: String?
        let runtimeRoot: String?
        let socketPath: String?
        let error: String?

        var testDescription: String { name }
    }

    struct Document: Decodable, Sendable {
        let fnv1a64: [Vector]
        let cases: [Case]
    }

    static func document() throws -> Document {
        try JSONDecoder().decode(Document.self, from: Fixture.data("layout/socket-paths.json"))
    }

    @Test func fnvVectorsMatchTheReferenceValues() throws {
        for vector in try Self.document().fnv1a64 {
            #expect(FNV1a.hex(FNV1a.hash64(Array(vector.input.utf8)), digits: 16) == vector.hex)
        }
    }

    @Test(arguments: try SocketPathTests.document().cases)
    func fixtureCaseGivesTheRustPath(_ fixture: Case) throws {
        // The fixture's override is already canonical; canonicalization has
        // its own test against the real file system.
        let resolve = {
            () throws(RootOverrideProblem) -> SocketEndpoint in
            try SocketPathResolver.resolve(
                rootOverride: fixture.override,
                userTempDir: Array(fixture.tempDir.utf8),
                canonicalize: { $0 }
            )
        }
        switch fixture.error {
        case "runtime_root_too_long":
            #expect(throws: RootOverrideProblem.self) { try resolve() }
            do {
                _ = try resolve()
            } catch {
                guard case .runtimeRootTooLong(let length) = error else {
                    Issue.record("unexpected \(error)")
                    return
                }
                #expect(length > SocketPathResolver.runtimeRootMax)
            }
        case nil:
            let endpoint = try resolve()
            #expect(endpoint.runtimeRoot == fixture.runtimeRoot)
            #expect(endpoint.socketPath == fixture.socketPath)
            #expect(endpoint.socketPathBytes == Array((fixture.socketPath ?? "").utf8))
            #expect(endpoint.rootOverride == (fixture.override != nil))
        case let other?:
            Issue.record("unknown fixture error \(other)")
        }
    }

    @Test func fixtureKeepsItsMinimumCases() throws {
        let names = Set(try Self.document().cases.map(\.name))
        for name in [
            "default_root", "default_root_without_trailing_slash", "override_ascii",
            "override_space_and_nfc", "override_nfd", "override_under_private_var",
            "runtime_root_76_bytes", "runtime_root_77_bytes",
        ] {
            #expect(names.contains(name), "missing \(name)")
        }
    }

    @Test func emptyAndRelativeOverridesAreRefusedBeforeCanonicalizing() {
        let temp = Array("/var/folders/zz/abc/T/".utf8)
        let neverCalled: ([UInt8]) throws(RootOverrideProblem) -> [UInt8] = { _ in
            Issue.record("canonicalize ran")
            return []
        }
        #expect(throws: RootOverrideProblem.empty) {
            try SocketPathResolver.resolve(
                rootOverride: "", userTempDir: temp, canonicalize: neverCalled)
        }
        #expect(throws: RootOverrideProblem.notAbsolute) {
            try SocketPathResolver.resolve(
                rootOverride: "stackly-root", userTempDir: temp, canonicalize: neverCalled)
        }
        #expect(throws: RootOverrideProblem.tempDirUnavailable) {
            try SocketPathResolver.resolve(
                rootOverride: nil, userTempDir: [], canonicalize: neverCalled)
        }
    }

    @Test func realPathResolvesSymlinksAndRefusesMissingFolders() throws {
        let folder = FileManager.default.temporaryDirectory
            .appending(path: "stackly-socket-path-\(UUID().uuidString.prefix(8))")
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: folder) }
        let real = folder.appending(path: "real root")
        let link = folder.appending(path: "link")
        try FileManager.default.createDirectory(at: real, withIntermediateDirectories: true)
        try FileManager.default.createSymbolicLink(at: link, withDestinationURL: real)

        let resolved = try SocketPathResolver.realPath(Array(link.path(percentEncoded: false).utf8))
        let expected = try SocketPathResolver.realPath(Array(real.path(percentEncoded: false).utf8))
        #expect(resolved == expected)
        #expect(String(decoding: resolved, as: UTF8.self).hasSuffix("/real root"))

        let missing = Array(folder.appending(path: "missing").path(percentEncoded: false).utf8)
        #expect(throws: RootOverrideProblem.doesNotExist) {
            try SocketPathResolver.realPath(missing)
        }
    }

    @Test func currentUsesTheUsersPrivateTemporaryFolder() throws {
        let temp = try DarwinTempDir.current()
        let endpoint = try SocketPathResolver.current(rootOverride: nil)
        let base = temp.last == UInt8(ascii: "/") ? Array(temp.dropLast()) : temp
        #expect(endpoint.socketPathBytes == base + Array("/stackly/stacklyd.sock".utf8))
        #expect(endpoint.rootOverride == false)
    }
}
