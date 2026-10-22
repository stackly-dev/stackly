import Foundation
import StacklyClient
import Testing

/// The duplicate DTOs of protocol 1.13 from the shared fixtures (spec 0024
/// AC-10): the two duplicate forms decode with every port `auto` and every
/// secret `generate`, the frames carry the PostgreSQL fixture, and foreign
/// values of the open enums read as `.unrecognized`.
struct DuplicateContractTests {
    static func form(_ name: String) throws -> DuplicateForm {
        try StacklyJSON.decoder().decode(
            DuplicateForm.self, from: Fixture.data("instance/duplicate-form-\(name).json"))
    }

    @Test func eachDuplicateFormAsksAutoPortsGeneratesSecretsAndPinsThePackage() throws {
        for name in ["fake", "postgresql"] {
            let duplicate = try Self.form(name)
            #expect(duplicate.form.provider.source == .pinned)
            #expect(duplicate.draft.runtimeId == duplicate.form.runtime.id)
            #expect(duplicate.draft.displayName == nil)
            #expect(duplicate.draft.start != true)
            let endpoints = duplicate.draft.endpoints ?? []
            #expect(!endpoints.isEmpty)
            #expect(endpoints.allSatisfy { $0.port == .auto })
            let secretFields = duplicate.form.fields.filter(\.secret).map(\.key)
            let plans = duplicate.draft.secrets ?? []
            #expect(plans.map(\.field) == secretFields)
            #expect(plans.allSatisfy { $0.source == .generate })
            #expect(duplicate.secrets.map(\.field) == secretFields)
            #expect(duplicate.secrets.allSatisfy { $0.sourceReference != nil })
            #expect(duplicate.form.suggestedName == duplicate.draft.name)
            for key in secretFields {
                #expect(duplicate.draft.settings.members[key] == nil)
            }
        }
    }

    @Test func theNamesFollowTheNamesAlreadyTaken() throws {
        let postgresql = try Self.form("postgresql")
        #expect(postgresql.source.name == "pg17-dev")
        #expect(postgresql.draft.name == "pg17-dev-copy")
        #expect(
            postgresql.secrets.first?.sourceReference
                == "local:instance/0192f3a6-0000-7000-8000-0000000000b2/superuser-password")
        let fake = try Self.form("fake")
        #expect(fake.source.name == "fake-a")
        #expect(fake.draft.name == "fake-a-copy-2")
    }

    @Test func theFramesCarryTheRequestAndThePostgresqlFixture() throws {
        let request = try Fixture.ipc(Request.self, "request-instance-get-duplicate-form.json")
        guard case .instanceGetDuplicateForm(let params) = request.call else {
            Issue.record("expected an instance.get_duplicate_form call")
            return
        }
        #expect(params.instanceId == UUID(uuidString: "0192f3a6-0000-7000-8000-0000000000b2"))

        guard
            case .instanceGetDuplicateForm(let form) = try CreateContractTests.reply(
                "frame-response-instance-get-duplicate-form.json")
        else {
            Issue.record("expected an instance.get_duplicate_form reply")
            return
        }
        #expect(form == (try Self.form("postgresql")))
    }

    @Test func foreignValuesOfTheDuplicateFormDecodeAsUnrecognized() throws {
        guard
            case .instanceGetDuplicateForm(let form) = try CreateContractTests.reply(
                "frame-response-instance-get-duplicate-form-unknown-values.json")
        else {
            Issue.record("expected an instance.get_duplicate_form reply")
            return
        }
        #expect(form.form.provider.source == .unrecognized)
        #expect(form.form.endpoints[0].purpose == .unrecognized)
        #expect(form.form.fields[0].kind == .unrecognized)
        #expect(form.form.fields[0].visibility == .unrecognized)
        #expect(form.form.fields[0].apply == .unrecognized)
        #expect(form.form.capabilities[0].capability == .unrecognized)
    }
}
