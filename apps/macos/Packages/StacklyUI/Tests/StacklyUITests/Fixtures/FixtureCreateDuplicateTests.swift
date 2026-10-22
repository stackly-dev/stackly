import Foundation
import StacklyClient
import Testing

@testable import StacklyUI

/// The duplicate form of the fixtures (spec 0024, added to spec 0068): it round trips through
/// the wire coders, names an instance the scenarios list, and shows the shape the daemon
/// promises (every port `auto`, every secret `generate`, a pinned package).
struct FixtureCreateDuplicateTests {
    private let form = FixtureCreateDuplicate.pg17

    @Test func theFormRoundTripsThroughTheWireCoders() throws {
        let data = try StacklyJSON.encoder().encode(form)
        #expect(try StacklyJSON.decoder().decode(DuplicateForm.self, from: data) == form)
    }

    @Test func theDraftAsksAutoPortsGeneratedSecretsAndAPinnedPackage() {
        #expect(form.form.provider.source == .pinned)
        #expect(form.draft.start == false)
        #expect(form.draft.displayName == nil)
        #expect(form.draft.name == form.form.suggestedName)
        #expect(form.draft.endpoints?.allSatisfy { $0.port == .auto } == true)
        #expect(form.draft.secrets?.allSatisfy { $0.source == .generate } == true)
        #expect(form.draft.runtimeId == form.form.runtime.id)
        #expect(form.draft.provider.revision == form.form.provider.revision)
        #expect(form.draft.settings.members["superuserPassword"] == nil)
        #expect(form.secrets.map(\.field) == form.form.fields.filter(\.secret).map(\.key))
    }

    @Test func theSourceIsAnInstanceTheScenariosList() throws {
        let holder = try #require(
            FixtureScenario.instanceCases.first { $0.id == FixtureCreateDuplicate.source.id })
        #expect(holder.name == FixtureCreateDuplicate.source.name)
        #expect(holder.runtimeId == form.draft.runtimeId)
        #expect(
            form.secrets.first?.sourceReference
                == "local:instance/\(holder.id.uuidString.lowercased())/superuser-password")
    }

    @Test func thePreflightOfTheDraftIsReadyAndNamesTheDuplicate() {
        let ready = FixtureCreateDuplicate.ready
        #expect(ready.ready)
        #expect(ready.checks.allSatisfy { $0.status == .ok })
        #expect(ready.checks.first { $0.id == "name" }?.detail == "pg17-dev-copy")
        #expect(ready.command.hasPrefix("stackly instance create pg17-dev-copy "))
        #expect(ready.command.contains("--port postgres=auto"))
    }

    @Test func theScenariosAnswerTheMethod() {
        #expect(Set(FixtureCreateDuplicate.replies.keys) == ["instance.get_duplicate_form"])
    }
}
