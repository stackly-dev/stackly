import Foundation
import StacklyClient
import Testing

/// The `runtime` area of protocol 1.11 from the shared fixtures (spec 0018
/// AC-18): the four replies and the runtime fixtures decode, foreign values
/// of the open enums read as `.unrecognized`, the unit event decodes, and
/// the runtime errors carry their details and static hints.
struct RuntimeContractTests {
    static func reply(_ file: String) throws -> Reply {
        guard case .response(let response) = try Fixture.ipc(ServerFrame.self, file) else {
            throw FixtureShapeError.unexpected(file)
        }
        return response.reply
    }

    static func runtime<Value: Decodable>(_ type: Value.Type, _ file: String) throws -> Value {
        try StacklyJSON.decoder().decode(type, from: Fixture.data("runtime/\(file)"))
    }

    @Test func theListReplyCarriesTheRuntimesAndTheInstallsThatHaveNotEnded() throws {
        guard case .runtimeList(let page) = try Self.reply("frame-response-runtime-list.json")
        else {
            Issue.record("expected a runtime.list reply")
            return
        }
        #expect(page == (try Self.runtime(RuntimeListPage.self, "list.json")))
        let items = try #require(page.items)
        #expect(items.map(\.software) == ["postgresql", "nats"])
        #expect(items.allSatisfy { $0.state == .installed && $0.installMode == .managed })
        #expect(items[0].support == .experimental)
        #expect(items[0].supportReason == .runtimeTested)
        #expect(items[0].usedBy?.map(\.name) == ["orders-db"])
        #expect(page.next == nil)
        let installing = try #require(page.installing?.first)
        #expect(installing.provider == "official/traefik")
        #expect(installing.sha256.count == 64)
    }

    @Test func theDetailReplyCarriesTheArtifactTheSignaturesAndTheCompatibilityRecords() throws {
        guard case .runtimeGet(let detail) = try Self.reply("frame-response-runtime-get.json")
        else {
            Issue.record("expected a runtime.get reply")
            return
        }
        #expect(detail == (try Self.runtime(RuntimeDetail.self, "detail-postgresql.json")))
        let artifact = try #require(detail.artifact)
        #expect(artifact.sha256 == detail.summary.artifactSha256)
        #expect(artifact.format == .tarGz)
        #expect(artifact.unpackedSize == 80_668_813)
        let signature = try #require(artifact.signature)
        #expect(signature.policy == .keepOrAdhoc)
        #expect(signature.executables?.first?.kind == .adhoc)
        #expect(artifact.dependencies?.map(\.kind) == [.bundled, .system])
        let record = try #require(detail.compatibility?.first)
        #expect(record.scope == .runtime)
        #expect(record.result == .pass)
        #expect(record.providerRevision == nil)
        #expect(record.knownLimitations == ["provider_integration_not_covered"])
    }

    @Test func anInstallStartsAnOperationOrAnswersThatTheArtifactIsInstalled() throws {
        let resolved = try Self.runtime(ResolvedArtifact.self, "resolved-postgresql.json")
        guard
            case .runtimeInstall(let started) = try Self.reply(
                "frame-response-runtime-install-started.json")
        else {
            Issue.record("expected a runtime.install reply")
            return
        }
        #expect(started.resolved == resolved)
        guard case .started(let accepted) = started.outcome else {
            Issue.record("expected a started outcome")
            return
        }
        #expect(accepted.operation.kind == .installRuntime)
        #expect(accepted.replayed == false)

        guard
            case .runtimeInstall(let done) = try Self.reply(
                "frame-response-runtime-install-already-installed.json")
        else {
            Issue.record("expected a runtime.install reply")
            return
        }
        guard case .alreadyInstalled(let summary) = done.outcome else {
            Issue.record("expected an alreadyInstalled outcome")
            return
        }
        #expect(summary.state == .installed)
        #expect(summary.artifactSha256 == resolved.sha256)
    }

    @Test func theResolveReplyIsTheResolvedArtifact() throws {
        guard
            case .runtimeResolve(let resolved) = try Self.reply(
                "frame-response-runtime-resolve.json")
        else {
            Issue.record("expected a runtime.resolve reply")
            return
        }
        #expect(resolved == (try Self.runtime(ResolvedArtifact.self, "resolved-postgresql.json")))
        #expect(resolved.stable)
        #expect(resolved.size == 28_311_215)
    }

    @Test func theRuntimeEventHasNoPayloadAndNamesTheRuntime() throws {
        guard
            case .event(let event) = try Fixture.ipc(
                ServerFrame.self, "frame-event-runtime-state-changed.json")
        else {
            Issue.record("expected an event frame")
            return
        }
        #expect(event.kind == .runtimeStateChanged)
        #expect(event.resource?.kind == .runtime)
        #expect(event.revision == 2)
    }

    @Test func foreignRuntimeValuesDecodeAsUnrecognized() throws {
        guard
            case .runtimeGet(let detail) = try Self.reply(
                "frame-response-runtime-get-unknown-values.json")
        else {
            Issue.record("expected a runtime.get reply")
            return
        }
        #expect(detail.summary.installMode == .unrecognized)
        #expect(detail.summary.state == .unrecognized)
        #expect(detail.summary.support == .unrecognized)
        #expect(detail.summary.supportReason == .unrecognized)
        #expect(detail.summary.target.os == .unrecognized)
        #expect(detail.summary.target.arch == .unrecognized)
        let artifact = try #require(detail.artifact)
        #expect(artifact.format == .unrecognized)
        #expect(artifact.signature?.policy == .unrecognized)
        #expect(artifact.signature?.executables?.first?.kind == .unrecognized)
        #expect(artifact.dependencies?.first?.kind == .unrecognized)
        let record = try #require(detail.compatibility?.first)
        #expect(record.scope == .unrecognized)
        #expect(record.result == .unrecognized)
        #expect(record.os == .unrecognized)
        #expect(record.arch == .unrecognized)

        guard
            case .runtimeResolve(let resolved) = try Self.reply(
                "frame-response-runtime-resolve-unknown-values.json")
        else {
            Issue.record("expected a runtime.resolve reply")
            return
        }
        #expect(resolved.format == .unrecognized)
        #expect(resolved.support == .unrecognized)
        #expect(resolved.supportReason == .unrecognized)
    }

    @Test func theRuntimeErrorsCarryTheirDetailsAndTheirStaticHint() throws {
        for (file, code, keys, hint) in [
            (
                "frame-error-runtime-selector-unresolved.json", "runtime.selector_unresolved",
                ["provider", "selector", "reason", "knownLines"],
                "Run stackly provider inspect for this provider to see its versions."
            ),
            (
                "frame-error-runtime-download-offline.json", "runtime.download_offline",
                ["artifact", "hosts"],
                "Connect to the internet and try again. Stackly keeps any partial download."
            ),
        ] {
            guard case .error(let frame) = try Fixture.ipc(ServerFrame.self, file) else {
                throw FixtureShapeError.unexpected(file)
            }
            #expect(frame.error.code == code)
            #expect(frame.error.details?.map(\.key) == keys)
            #expect(frame.error.hint == hint)
        }
    }

    @Test func theNewSupportReasonsOfTheCompatibilityRulesAreKnownValues() throws {
        let decoder = StacklyJSON.decoder()
        for (text, reason) in [
            ("requires_newer_os", SupportReason.requiresNewerOs),
            ("provider_tested", .providerTested),
            ("runtime_tested", .runtimeTested),
        ] {
            #expect(
                try decoder.decode(SupportReason.self, from: Data("\"\(text)\"".utf8)) == reason)
        }
    }
}
