import Foundation
import StacklyClient
import Testing

@testable import StacklyUI

/// The words and the small rules of the New Instance sheet (spec 0020 AC-20 to AC-22): the
/// field error table, the preflight status mapping, the labels VoiceOver reads, and the
/// pieces of the draft that need no view.
struct CreateInstanceCopyTests {
    private func field(
        _ kind: FieldKind, managed: Bool = false, minLength: UInt64? = nil,
        maxLength: UInt64? = nil, minimum: Int64? = nil, maximum: Int64? = nil,
        minItems: UInt64? = nil, maxItems: UInt64? = nil, itemKey: String? = nil
    ) -> FormField {
        FormField(
            key: "setting", title: "Setting", description: nil, kind: kind, required: false,
            secret: false, printableOnly: false, managed: managed, visibility: .basic,
            apply: nil, default: nil, minLength: minLength, maxLength: maxLength,
            minimum: minimum, maximum: maximum, choices: [], minItems: minItems,
            maxItems: maxItems, itemKey: itemKey, items: [])
    }

    private func issue(_ code: String, path: String = "settings.setting") -> FieldError {
        FieldError(path: path, code: code, message: "fixed")
    }

    // MARK: - Field errors

    @Test func everyRowOfTheFieldErrorTableHasItsSentence() {
        func text(_ code: String, _ field: FormField, path: String = "settings.setting")
            -> String
        {
            FieldErrorText.settings(issue(code, path: path), field: field)
        }
        #expect(text("unknown_value", field(.text)) == "Unknown setting.")
        #expect(
            text("unknown_value", field(.choice)) == "Choose one of the listed values.")
        #expect(text("invalid_value", field(.integer)) == "Use a whole number.")
        #expect(text("invalid_value", field(.boolean)) == "Use a true or false.")
        #expect(
            text("invalid_value", field(.text, managed: true)) == "Set by Stackly; leave it out.")
        #expect(
            text(
                "invalid_value", field(.list, itemKey: "host"), path: "settings.setting[1].host")
                == "Each host must be unique.")
        #expect(
            text("invalid_format", field(.text)) == "Doesn’t match the required format.")
        #expect(text("too_short", field(.text, minLength: 3)) == "Use at least 3 characters.")
        #expect(text("too_long", field(.text, maxLength: 9)) == "Use at most 9 characters.")
        #expect(
            text("out_of_range", field(.integer, minimum: 5, maximum: 10))
                == "Use a whole number from 5 to 10.")
        #expect(text("too_many", field(.list, maxItems: 4)) == "Use at most 4 items.")
        #expect(text("too_short", field(.list, minItems: 2)) == "Use at least 2 items.")
        #expect(text("required", field(.text)) == "Required.")
    }

    @Test func aFieldErrorWithoutItsFieldFallsBackToAGeneralSentence() {
        #expect(FieldErrorText.settings(issue("too_long"), field: nil) == "Check this value.")
    }

    @Test func theNameErrorsNameTheRuleOrTheInstanceThatHoldsTheName() {
        #expect(
            FieldErrorText.name(issue("invalid_format", path: "name"), name: "Bad_Name")
                == "Use 1 to 63 lowercase letters, digits, or hyphens, starting with a letter.")
        #expect(
            FieldErrorText.name(issue("invalid_value", path: "name"), name: "orders-db")
                == "An instance named orders-db already exists.")
    }

    // MARK: - Preflight rows

    @Test func everyPreflightStatusMapsToTheStatusOfTheChecklist() {
        #expect(PreflightMapping.status(of: .ok) == .passed)
        #expect(PreflightMapping.status(of: .caution) == .warning)
        #expect(PreflightMapping.status(of: .error) == .failed)
        #expect(PreflightMapping.status(of: .unrecognized) == .unrecognized)
    }

    @Test func theRowsKeepTheDaemonsIdTitleAndDetail() {
        let items = PreflightMapping.items(FixtureCreate.portConflict.checks)
        #expect(items.map(\.id) == FixtureCreate.portConflict.checks.map(\.id))
        let taken = items.first { $0.id == "endpoint.postgres" }
        #expect(taken?.status == .failed)
        #expect(taken?.detail == "In use by pg17-dev")
        #expect(taken?.accessibilityLabel == "Endpoint 127.0.0.1:5432, Failed: In use by pg17-dev")
    }

    @Test func aSecretRowReadsStoredOnceTheSecretsStageIsDone() {
        let checks = FixtureCreateForm.ready.checks
        let stored = PreflightMapping.items(checks, secretsAreStored: true)
        let secret = stored.first { $0.id == "secret.superuserPassword" }
        #expect(secret?.detail == "Stored in Keychain")
        #expect(stored.first { $0.id == "disk" }?.detail == "41 GB free · needs at least 256 MB")
    }

    @Test func theRowsOfAFailedStageShowAsFailed() {
        let items = PreflightMapping.items(
            FixtureCreateForm.ready.checks, failedKinds: [.secret])
        #expect(items.first { $0.id == "secret.superuserPassword" }?.status == .failed)
        #expect(items.first { $0.id == "disk" }?.status == .passed)
    }

    // MARK: - Labels

    @Test func aFieldIsReadAsItsTitleRequiredAndSetOnce() throws {
        let form = try contractForm("postgresql")
        let schema = SchemaFormModel(fields: form.fields)
        let password = try #require(schema.row("superuserPassword"))
        #expect(password.spokenLabel == "Superuser password, required, set once at creation")
        let log = try #require(schema.row("logStatement"))
        #expect(log.spokenLabel == "Log statements")
    }

    @Test func theCategoryFilterLeavesTheTestingCategoryToAll() {
        #expect(CreateCategory.all.includes(.testing))
        #expect(CreateCategory.databases.includes(.testing) == false)
        #expect(CreateCategory.databases.includes(.database))
        #expect(CreateCategory.all.includes(nil))
        #expect(CreateCategory.cache.includes(nil) == false)
        #expect(
            CreateCategory.allCases.map(\.title) == [
                "All", "Databases", "Messaging", "Analytics", "Cache", "Proxy",
            ])
    }

    @Test func theCopyFromLabelCapitalisesThePurposeAndDropsTheHyphens() {
        #expect(CreateInstanceModel.purposeTitle("superuser-password") == "Superuser password")
        #expect(CreateInstanceModel.purposeTitle("admin") == "Admin")
    }

    @Test func aReferenceOfStacklyItselfIsNeverOffered() {
        #expect(CreateInstanceModel.isReserved("local:stackly/internal"))
        #expect(CreateInstanceModel.isReserved("local:instance/abc/password") == false)
        #expect(CreateInstanceModel.isReserved("local:user/stackly") == false)
    }

    @Test func theSummaryNamesFailuresAndWarningsInTheWordsOfTheSpec() {
        func summary(_ preflight: CreatePreflight) -> String {
            CreateInstanceModel.summary(of: preflight)
        }
        #expect(summary(FixtureCreateForm.ready) == "Preflight passed.")
        #expect(summary(FixtureCreateForm.caution) == "Preflight passed with 1 warning.")
        #expect(summary(FixtureCreate.portConflict) == "1 preflight check failed.")
    }

    // MARK: - The draft

    @Test func anEndpointStartsFixedOnItsDefaultPortAndAutomaticWithoutOne() {
        let withPort = EndpointDraft(
            CreateEndpoint(
                name: "client", transport: .tcp, purpose: .client, host: "127.0.0.1",
                defaultPort: 4222, socketName: nil))
        #expect(withPort.mode == .fixed)
        #expect(withPort.portText == "4222")
        #expect(withPort.request == EndpointRequest(name: "client", port: .fixed(4222)))
        let without = EndpointDraft(
            CreateEndpoint(
                name: "client", transport: .tcp, purpose: .client, host: nil, defaultPort: nil,
                socketName: nil))
        #expect(without.mode == .automatic)
        #expect(without.request == EndpointRequest(name: "client", port: .auto))
    }

    @Test func aPortOutsideTheWireRangeIsNotSent() {
        var draft = EndpointDraft(
            name: "client", purpose: .client, host: nil, mode: .fixed, portText: "65536")
        #expect(draft.request == nil)
        #expect(draft.hasInvalidPort)
        draft.portText = " 8080 "
        #expect(draft.port == 8080)
        draft.mode = .automatic
        draft.portText = "abc"
        #expect(draft.hasInvalidPort == false)
    }

    @Test func theIdentifiersFollowTheScopePartKeyPattern() {
        let ids = [
            AccessibilityID.CreateInstance.category, AccessibilityID.CreateInstance.runtime,
            AccessibilityID.CreateInstance.name,
            AccessibilityID.CreateInstance.portMode("client"),
            AccessibilityID.CreateInstance.port("client"),
            AccessibilityID.CreateInstance.usePort("client"),
            AccessibilityID.CreateInstance.field("jetstream"),
            AccessibilityID.CreateInstance.advanced,
            AccessibilityID.CreateInstance.startAfterCreating,
            AccessibilityID.CreateInstance.preflight,
            AccessibilityID.CreateInstance.preflightRetry,
            AccessibilityID.CreateInstance.command, AccessibilityID.CreateInstance.progress,
            AccessibilityID.CreateInstance.banner,
        ]
        #expect(Set(ids).count == ids.count)
        #expect(ids.allSatisfy { $0.hasPrefix("createInstance.") && !$0.contains(" ") })
    }

    @Test func theDraftOfTheNatsFormHasItsTwoPortRowsAndNoUnixRow() async throws {
        let form = try contractForm("nats")
        let rig = CreateRig()
        rig.service.onForm = { _ in form }
        await rig.load()
        rig.model.begin()
        await settle("the form") { rig.model.phase == .editing }
        #expect(rig.model.endpoints.map(\.name) == ["client", "monitoring"])
        #expect(rig.model.endpoints.map(\.portText) == ["4222", "8222"])
        #expect(rig.model.schema?.row("serverName")?.control == .managed)
        #expect(rig.model.schema?.row("jetstream")?.control == .toggle)
        #expect(rig.model.schema?.advancedRows.count == 3)
    }
}
