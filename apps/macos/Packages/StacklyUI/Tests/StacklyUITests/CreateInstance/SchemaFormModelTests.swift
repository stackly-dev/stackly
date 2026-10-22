import Foundation
import StacklyClient
import Testing

@testable import StacklyUI

/// The settings form built from the three create forms of the shared contract (spec 0020
/// AC-21, AC-1): one row per field with the right control, and nothing that names a provider.
struct SchemaFormModelTests {
    private func schema(_ name: String) throws -> SchemaFormModel {
        SchemaFormModel(fields: try contractForm(name).fields)
    }

    @Test func theNatsFormHasOneRowPerFieldWithTheRightControl() throws {
        let schema = try schema("nats")
        #expect(
            schema.rows.map(\.id) == [
                "serverName", "jetstream", "jetstreamMaxMemoryBytes", "jetstreamMaxFileBytes",
                "maxPayloadBytes",
            ])
        #expect(schema.rows[0].control == .managed)
        #expect(schema.rows[1].control == .toggle)
        for row in schema.rows.dropFirst(2) {
            #expect(row.control == .integer)
            #expect(row.isAdvanced)
            #expect(row.boundsHint != nil)
        }
        #expect(schema.basicRows.map(\.id) == ["serverName", "jetstream"])
        #expect(schema.advancedRows.count == 3)
    }

    @Test func theNatsManagedFieldIsLockedAndNeverSent() throws {
        let schema = try schema("nats")
        let members = schema.settings().members
        #expect(members["serverName"] == nil)
        #expect(members["jetstream"] == .bool(true))
        #expect(members["maxPayloadBytes"] == .number(1_048_576))
    }

    @Test func theNatsAdvancedIntegersCarryTheirBounds() throws {
        let schema = try schema("nats")
        let memory = try #require(schema.row("jetstreamMaxMemoryBytes"))
        #expect(memory.field.minimum == 1_048_576)
        #expect(memory.field.maximum == 1_099_511_627_776)
        #expect(memory.boundsHint == "From 1048576 to 1099511627776.")
        #expect(memory.isSetOnce == false)
    }

    @Test func aSecretFieldTakesItsLimitsFromTheDatabaseForm() throws {
        let schema = try schema("postgresql")
        let row = try #require(schema.rows.first { $0.id == "superuserPassword" })
        #expect(
            row.control
                == .secret(SecretLimits(minLength: 8, maxLength: 128, printableOnly: true)))
        #expect(row.isSetOnce)
        #expect(row.showsRequired)
        #expect(schema.settings().members["superuserPassword"] == nil)
    }

    @Test func aChoiceFieldOffersItsValuesAndStartsOnItsDefault() throws {
        let schema = try schema("postgresql")
        let row = try #require(schema.row("logStatement"))
        #expect(row.control == .choice(["none", "ddl", "mod", "all"]))
        #expect(schema.text("logStatement") == "none")
    }

    @Test func theFakeFormHasASecretAndBasicAndAdvancedFields() throws {
        let schema = try schema("fake")
        #expect(schema.rows.count == 10)
        #expect(schema.rows.last?.control == .secret(SecretLimits(minLength: 1, maxLength: 256)))
        #expect(schema.rows.filter(\.isAdvanced).count == 6)
        #expect(schema.row("failInit")?.isSetOnce == true)
        #expect(schema.row("failInit")?.control == .toggle)
    }

    @Test func aFieldOfAnUnknownKindIsAReadOnlyRow() {
        let field = Self.field(key: "later", kind: .unrecognized)
        let schema = SchemaFormModel(fields: [field])
        #expect(schema.rows.first?.control == .unrecognized)
        #expect(schema.settings().members.isEmpty)
    }

    @Test func aTextThatIsEmptyIsLeftOutAndAnIntegerThatIsNotANumberGoesAsText() {
        let schema = SchemaFormModel(fields: [
            Self.field(key: "label", kind: .text),
            Self.field(key: "limit", kind: .integer),
        ])
        #expect(schema.settings().members.isEmpty)
        schema.setText("limit", "12")
        #expect(schema.settings().members["limit"] == .number(12))
        schema.setText("limit", "abc")
        #expect(schema.settings().members["limit"] == .string("abc"))
    }

    @Test func listItemsAddAndRemoveWithinTheirBounds() {
        let item = FormItemField(
            key: "host", title: "Host", description: nil, kind: .text, required: true,
            default: .text("localhost"), minLength: nil, maxLength: nil, minimum: nil,
            maximum: nil, choices: [])
        var field = Self.field(key: "hosts", kind: .list)
        field = FormField(
            key: field.key, title: field.title, description: nil, kind: .list, required: false,
            secret: false, printableOnly: false, managed: false, visibility: .basic, apply: nil,
            default: nil, minLength: nil, maxLength: nil, minimum: nil, maximum: nil,
            choices: [], minItems: 1, maxItems: 2, itemKey: "host", items: [item])
        let schema = SchemaFormModel(fields: [field])
        schema.addItem(to: "hosts")
        schema.addItem(to: "hosts")
        schema.addItem(to: "hosts")
        #expect(schema.items("hosts").count == 2)
        #expect(
            schema.settings().members["hosts"]
                == .array([
                    .object(["host": .string("localhost")]),
                    .object(["host": .string("localhost")]),
                ]))
        let first = schema.items("hosts")[0].id
        schema.deleteItem(from: "hosts", id: first)
        schema.deleteItem(from: "hosts", id: schema.items("hosts")[0].id)
        #expect(schema.items("hosts").count == 1)
    }

    @Test func issuesAtAFieldTurnIntoTheSentencesOfTheTable() throws {
        let schema = try schema("nats")
        schema.apply(issues: [
            FieldError(
                path: "settings.maxPayloadBytes", code: "out_of_range", message: "x"),
            FieldError(path: "settings.unknown", code: "unknown_value", message: "x"),
            FieldError(path: "settings", code: "invalid_value", message: "x"),
        ])
        #expect(
            schema.errorTexts(for: "maxPayloadBytes") == [
                "Use a whole number from 1024 to 67108864."
            ])
        #expect(schema.advancedHasIssue)
        #expect(schema.errorTexts(for: "jetstream").isEmpty)
        #expect(schema.unmatchedIssues.map(\.path) == ["settings.unknown", "settings"])
    }

    @Test func aChangeTellsTheOwnerButTypingASecretDoesNot() throws {
        let schema = try schema("postgresql")
        var changes = 0
        schema.onChange = { changes += 1 }
        schema.setText("logStatement", "none")
        #expect(changes == 0)
        schema.setText("logStatement", "ddl")
        #expect(changes == 1)
        schema.setSecret(
            "superuserPassword", SecretChoice(source: .enter, reference: nil, isValid: false))
        #expect(changes == 2)
        schema.setSecret(
            "superuserPassword", SecretChoice(source: .enter, reference: nil, isValid: true))
        #expect(changes == 2)
        #expect(schema.secretPlans() == [SecretPlan(field: "superuserPassword", source: .value)])
        schema.setSecret(
            "superuserPassword",
            SecretChoice(source: .copyFrom, reference: "local:user/x", isValid: true))
        #expect(changes == 3)
        #expect(
            schema.secretPlans() == [
                SecretPlan(field: "superuserPassword", source: .copyFrom("local:user/x"))
            ])
    }

    static func field(key: String, kind: FieldKind) -> FormField {
        FormField(
            key: key, title: key, description: nil, kind: kind, required: false, secret: false,
            printableOnly: false, managed: false, visibility: .basic, apply: nil, default: nil,
            minLength: nil, maxLength: nil, minimum: nil, maximum: nil, choices: [],
            minItems: nil, maxItems: nil, itemKey: nil, items: [])
    }
}
