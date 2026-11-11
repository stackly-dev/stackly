import Foundation
import StacklyClient
import Testing

@testable import StacklyUI

/// The settings form of every package (spec 0036 AC-8, spec 0020 AC-21): each
/// `protocols/fixtures/instance/create-form-*.json` builds a `SchemaFormModel` with one row
/// per field and no field of a kind this build does not know. The files come from the
/// fixture folder, so a new package adds a fixture and no Swift code.
struct EveryCreateFormFixtureTests {
    private static func fixtureNames() throws -> [String] {
        let folder = SourceTree.url(of: "protocols/fixtures/instance")
        return try FileManager.default.contentsOfDirectory(
            atPath: folder.path(percentEncoded: false)
        )
        .filter { $0.hasPrefix("create-form-") && $0.hasSuffix(".json") }
        .map { String($0.dropFirst("create-form-".count).dropLast(".json".count)) }
        .sorted()
    }

    @Test func everyCreateFormFixtureBuildsASchemaFormWithOnlyKnownFieldKinds() throws {
        let names = try Self.fixtureNames()
        #expect(names.count >= 5, "only \(names) found")
        for name in names {
            let form = try contractForm(name)
            let schema = SchemaFormModel(fields: form.fields)
            #expect(schema.rows.map(\.id) == form.fields.map(\.key), "\(name)")
            for field in form.fields {
                #expect(field.kind != .unrecognized, "\(name): \(field.key) has an unknown kind")
            }
            for row in schema.rows {
                #expect(row.control != .unrecognized, "\(name): \(row.id) has no known control")
            }
        }
    }
}
