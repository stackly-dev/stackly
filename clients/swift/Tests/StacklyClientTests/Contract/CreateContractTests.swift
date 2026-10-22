import Foundation
import StacklyClient
import Testing

/// The create DTOs of protocol 1.12 from the shared fixtures (spec 0020
/// AC-1, AC-17, AC-21): the three create forms decode in the order the
/// package declares, foreign values of the open enums read as
/// `.unrecognized`, the preflight rows carry their errors, and the new
/// error codes carry their details and static hints.
struct CreateContractTests {
    static func reply(_ file: String) throws -> Reply {
        guard case .response(let response) = try Fixture.ipc(ServerFrame.self, file) else {
            throw FixtureShapeError.unexpected(file)
        }
        return response.reply
    }

    static func form(_ name: String) throws -> CreateForm {
        try StacklyJSON.decoder().decode(
            CreateForm.self, from: Fixture.data("instance/create-form-\(name).json"))
    }

    @Test func eachRealPackageDecodesAsACreateFormInDeclarationOrder() throws {
        let fake = try Self.form("fake")
        #expect(fake.provider.name == "testing/fake")
        #expect(fake.provider.category == .testing)
        let nats = try Self.form("nats")
        #expect(nats.provider.name == "official/nats")
        #expect(nats.endpoints.map(\.name) == ["client", "monitoring"])
        #expect(nats.endpoints.map(\.defaultPort) == [4222, 8222])
        #expect(nats.endpoints.allSatisfy { $0.transport == .tcp })
        #expect(
            nats.fields.map(\.key)
                == [
                    "serverName", "jetstream", "jetstreamMaxMemoryBytes",
                    "jetstreamMaxFileBytes", "maxPayloadBytes",
                ])
        #expect(nats.fields[0].managed)
        #expect(nats.fields[1].kind == .boolean)
        #expect(nats.fields[1].default == .boolean(true))
        for field in nats.fields.dropFirst(2) {
            #expect(field.kind == .integer)
            #expect(field.visibility == .advanced)
            #expect(field.minimum != nil && field.maximum != nil)
        }
        let postgresql = try Self.form("postgresql")
        let secret = try #require(postgresql.fields.first { $0.secret })
        #expect(secret.printableOnly)
        #expect(secret.minLength == 8 && secret.maxLength == 128)
        #expect(secret.default == nil)
        #expect(postgresql.dataPath == "instances/<new instance id>/data")
        #expect(postgresql.capabilities.count == 8)
    }

    @Test func theFramesCarryTheFormAndTheFixtureOfThePostgresqlPackage() throws {
        guard
            case .instanceGetCreateForm(let form) = try Self.reply(
                "frame-response-instance-get-create-form.json")
        else {
            Issue.record("expected an instance.get_create_form reply")
            return
        }
        #expect(form == (try Self.form("postgresql")))
    }

    @Test func foreignValuesOfTheCreateEnumsDecodeAsUnrecognized() throws {
        guard
            case .instanceGetCreateForm(let form) = try Self.reply(
                "frame-response-instance-get-create-form-unknown-values.json")
        else {
            Issue.record("expected an instance.get_create_form reply")
            return
        }
        #expect(form.provider.category == .unrecognized)
        #expect(form.provider.source == .unrecognized)
        #expect(form.endpoints[0].transport == .unrecognized)
        #expect(form.endpoints[0].purpose == .unrecognized)
        #expect(form.fields[0].kind == .unrecognized)
        #expect(form.fields[1].visibility == .unrecognized)
        #expect(form.fields[2].apply == .unrecognized)
        #expect(form.capabilities[0].capability == .unrecognized)

        guard
            case .instancePreflight(let preflight) = try Self.reply(
                "frame-response-instance-preflight-unknown-values.json")
        else {
            Issue.record("expected an instance.preflight reply")
            return
        }
        #expect(preflight.checks[0].kind == .unrecognized)
        #expect(preflight.checks[1].status == .unrecognized)
    }

    @Test func aPortConflictRowCarriesTheSuggestedPortAndTheErrorOfCreate() throws {
        guard
            case .instancePreflight(let preflight) = try Self.reply(
                "frame-response-instance-preflight-port-conflict.json")
        else {
            Issue.record("expected an instance.preflight reply")
            return
        }
        #expect(!preflight.ready)
        let row = try #require(preflight.checks.first { $0.id == "endpoint.postgres" })
        #expect(row.status == .error)
        #expect(row.suggestedPort == 5433)
        let error = try #require(row.error)
        #expect(error.code == "supervisor.endpoint_conflict")
        #expect(error.fields?.map(\.path) == preflight.fields.map(\.path))
        #expect(preflight.command.hasPrefix("stackly instance create orders-db"))

        guard
            case .instancePreflight(let ready) = try Self.reply(
                "frame-response-instance-preflight-ready.json")
        else {
            Issue.record("expected an instance.preflight reply")
            return
        }
        #expect(ready.ready)
        #expect(ready.checks.contains { $0.status == .caution })
        #expect(ready.checks.allSatisfy { $0.error == nil })
    }

    @Test func aPreflightRequestCarriesTheSourceOfEachSecretAndNeverAValue() throws {
        let request = try Fixture.ipc(Request.self, "request-instance-preflight.json")
        guard case .instancePreflight(let params) = request.call else {
            Issue.record("expected an instance.preflight request")
            return
        }
        #expect(params.start == true)
        #expect(
            (params.secrets ?? []).map(\.source)
                == [
                    .generate, .value, .copyFrom("local:erp/db/audit"),
                    .bind("local:erp/db/backup"),
                ])
        #expect((params.endpoints ?? []).map(\.name) == ["postgres", "admin"])
    }

    @Test func theNewErrorsCarryTheirDetailsAndTheirStaticHint() throws {
        for (file, code, keys, hint) in [
            (
                "frame-error-instance-name-invalid.json", "instance.name_invalid", ["omitted"],
                "use lowercase letters, digits, and single hyphens, starting with a letter"
            ),
            (
                "frame-error-instance-provider-unavailable.json",
                "instance.provider_unavailable", ["provider", "state"],
                "this provider is not in this release"
            ),
            (
                "frame-error-instance-capability-unsupported.json",
                "instance.capability_unsupported", ["capability", "provider", "existing"],
                "remove the existing instance first"
            ),
            (
                "frame-error-instance-runtime-incompatible.json",
                "instance.runtime_incompatible", ["runtimeId", "executable"],
                "reinstall the runtime with stackly runtime install"
            ),
            (
                "frame-error-instance-storage-unsupported.json",
                "instance.storage_unsupported", ["fsType", "root"],
                "keep the Stackly data folder on a local APFS volume"
            ),
            (
                "frame-error-instance-insufficient-space.json",
                "instance.insufficient_space", ["needed", "available"],
                "free up disk space, then try again"
            ),
            (
                "frame-error-instance-data-not-empty.json", "instance.data_not_empty", ["path"],
                "run stackly doctor; Stackly never initializes a folder that already holds data"
            ),
            (
                "frame-error-instance-data-overlap.json", "instance.data_overlap",
                ["path", "otherInstance"], "run stackly doctor"
            ),
            (
                "frame-error-instance-short-key-taken.json", "instance.short_key_taken",
                ["shortKey"], "try again, a new ID is generated"
            ),
            (
                "frame-error-catalog-pin-failed.json", "catalog.pin_failed",
                ["provider", "revision", "reason"], "run stackly doctor"
            ),
        ] {
            guard case .error(let frame) = try Fixture.ipc(ServerFrame.self, file) else {
                throw FixtureShapeError.unexpected(file)
            }
            #expect(frame.error.code == code)
            #expect(frame.error.details?.map(\.key) == keys)
            #expect(frame.error.hint == hint)
        }
        guard
            case .error(let frame) = try Fixture.ipc(
                ServerFrame.self, "frame-error-registry-not-deletable.json")
        else {
            throw FixtureShapeError.unexpected("frame-error-registry-not-deletable.json")
        }
        #expect(frame.error.code == "registry.not_deletable")
        #expect(frame.error.resource?.kind == .instance)
    }

    @Test func aSignedSafeIntegerTravelsAsInt64() throws {
        let decoder = StacklyJSON.decoder()
        let low = try decoder.decode(
            SettingValue.self, from: Data(#"{"type":"integer","content":-9007199254740991}"#.utf8))
        #expect(low == .integer(-9_007_199_254_740_991))
    }
}
