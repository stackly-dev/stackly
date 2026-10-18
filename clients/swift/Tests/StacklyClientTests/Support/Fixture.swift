import Foundation
import StacklyClient
import Testing

/// Shared JSON fixtures in `protocols/fixtures/` at the repository root.
enum Fixture {
    /// The repository root, found from this file's path. Builds must never
    /// remap source paths (`-file-prefix-map`), or this breaks.
    static let repositoryRoot = URL(filePath: #filePath)
        .deletingLastPathComponent()  // Fixture.swift
        .deletingLastPathComponent()  // Support
        .deletingLastPathComponent()  // StacklyClientTests
        .deletingLastPathComponent()  // Tests
        .deletingLastPathComponent()  // swift
        .deletingLastPathComponent()  // clients

    /// Reads one fixture file by path, for example `protocol-version.json`
    /// or `ipc/index.json`.
    static func data(_ name: String) throws -> Data {
        try Data(contentsOf: repositoryRoot.appending(path: "protocols/fixtures/\(name)"))
    }

    /// Reads a file by its path from the repository root.
    static func repositoryFile(_ path: String) throws -> Data {
        try Data(contentsOf: repositoryRoot.appending(path: path))
    }

    /// Decodes one fixture in `protocols/fixtures/ipc/` with `StacklyJSON`.
    static func ipc<Value: Decodable>(_ type: Value.Type, _ file: String) throws -> Value {
        try StacklyJSON.decoder().decode(type, from: data("ipc/\(file)"))
    }
}

/// One case in `protocols/fixtures/ipc/index.json`.
struct FixtureCase: Decodable, Sendable, CustomTestStringConvertible {
    /// The file in `protocols/fixtures/ipc/`.
    let file: String
    /// The Rust type name the file decodes as.
    let type: String
    /// `roundtrip`, `decode`, `skipped`, or `reject`.
    let expect: String
    /// The error code Rust checks for a `reject` case.
    let code: String?
    /// The languages the case applies to; every language when absent.
    let langs: [String]?

    var testDescription: String { file }

    /// Whether the Swift tests run this case.
    var appliesToSwift: Bool { langs?.contains("swift") ?? true }

    /// Every case in the index that applies to Swift.
    static func swiftCases() throws -> [FixtureCase] {
        struct Index: Decodable {
            let fixtures: [FixtureCase]
        }
        let index = try JSONDecoder().decode(Index.self, from: Fixture.data("ipc/index.json"))
        return index.fixtures.filter(\.appliesToSwift)
    }
}
