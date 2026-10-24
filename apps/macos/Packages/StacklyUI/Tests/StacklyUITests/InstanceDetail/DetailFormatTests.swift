import Foundation
import StacklyClient
import Testing

@testable import StacklyUI

/// The rows the detail page adds (spec 0021, *Format bersama* and AC-7): the same data as the
/// CLI goldens of `stackly instance inspect`, said by the GUI rules.
struct DetailFormatTests {
    private func triple(_ name: String) throws -> DetailFixture.Triple {
        try DetailFixture.triple(name)
    }

    // MARK: - Sizes

    @Test func sizesUseTheCliRule() {
        #expect(DetailFormat.bytes(0) == "0 bytes")
        #expect(DetailFormat.bytes(999) == "999 bytes")
        #expect(DetailFormat.bytes(1_000) == "1.0 kB")
        #expect(DetailFormat.bytes(1_250) == "1.2 kB")
        #expect(DetailFormat.bytes(999_949) == "999.9 kB")
        #expect(DetailFormat.bytes(999_950) == "1.0 MB")
        #expect(DetailFormat.bytes(1_288_490_188) == "1.3 GB")
        #expect(DetailFormat.bytes(52_428_800) == "52.4 MB")
        #expect(DetailFormat.bytes(UInt64.max) == "18446744.1 TB")
    }

    @Test func theDataSizeRowReadsEachStateOfTheCall() throws {
        let complete = try #require(try triple("postgresql").dataUsage)
        let partial = try #require(try triple("unavailable").dataUsage)

        #expect(DetailFormat.dataSize(.loaded(complete)) == "1.3 GB")
        #expect(DetailFormat.dataSize(.loaded(partial)) == "at least 52.4 MB")
        #expect(DetailFormat.dataSize(.loaded(nil)) == "—")
        #expect(DetailFormat.dataSize(.loading) == "Calculating…")
        #expect(DetailFormat.dataSize(.unavailable) == "Unavailable")
    }

    @Test func theScanAgeIsRelative() throws {
        let usage = try #require(try triple("postgresql").dataUsage)
        let now = usage.scannedAt.addingTimeInterval(180)
        #expect(DetailFormat.scanned(usage, now: now) == "scanned 3m ago")
    }

    // MARK: - Rows

    @Test func theRowsOfThePostgresqlTripleReadLikeTheCliGolden() throws {
        let item = try triple("postgresql")
        let detail = item.detail

        #expect(DetailFormat.digest(detail.runtime) == "sha256:a1b2c3d4e5f6")
        #expect(
            DetailFormat.fullDigest(detail.runtime)
                == "sha256:a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f90")
        #expect(DetailFormat.autostart(detail.startup) == "At login")
        #expect(DetailFormat.provider(detail.provider) == "official/postgresql 2026.10.1")
        #expect(
            DetailFormat.version(
                software: item.instance.software, detail: detail,
                runtimeVersion: item.instance.runtimeVersion) == "PostgreSQL 17.2.0 · arm64")
        #expect(
            detail.dependents.map(DetailFormat.dependent) == [
                "erp-dev · Dedicated · lease active", "reporting · Shared",
            ])
        #expect(detail.secrets.map(DetailFormat.secretNote) == [nil])
    }

    @Test func anExternalRuntimeHasNoDigestAndNothingDependsOnIt() throws {
        let item = try triple("nats")
        #expect(DetailFormat.digest(item.detail.runtime) == "External runtime")
        #expect(DetailFormat.fullDigest(item.detail.runtime) == nil)
        #expect(DetailFormat.autostart(item.detail.startup) == "Off")
        #expect(item.detail.dependents.isEmpty)
    }

    @Test func aMissingSecretSaysStartingAsksForIt() throws {
        let detail = try triple("unavailable").detail
        #expect(
            detail.secrets.map(DetailFormat.secretNote) == ["Missing; starting asks for it."])
        #expect(
            DetailFormat.missingValueReason("local:x") == "local:x has no stored value yet.")
    }

    @Test func theVersionFallsBackToTheSummaryBeforeTheDetailLoads() throws {
        let item = try triple("postgresql")
        #expect(
            DetailFormat.version(
                software: "postgresql", detail: nil, runtimeVersion: "17.2.0")
                == "postgresql 17.2.0")
        _ = item
    }

    @Test func theHomeFolderIsWrittenAsATilde() {
        #expect(
            DetailFormat.dataPath("/Users/ann/Library/Stackly/data", home: "/Users/ann")
                == "~/Library/Stackly/data")
        #expect(DetailFormat.dataPath("/Users/ann", home: "/Users/ann/") == "~")
        #expect(
            DetailFormat.dataPath("/Users/annette/data", home: "/Users/ann")
                == "/Users/annette/data")
        #expect(DetailFormat.dataPath("/Volumes/x/data", home: "") == "/Volumes/x/data")
    }

    @Test func restartAttemptsShowOnlyWhenThereWereSome() throws {
        let item = try triple("postgresql")
        #expect(DetailFormat.restartAttempts(item.instance.restart) == nil)
        let some = InstanceRestart(
            attempts: 2, budget: 5, windowSeconds: 60, nextAttemptAt: nil, exhausted: false)
        let spent = InstanceRestart(
            attempts: 5, budget: 5, windowSeconds: 60, nextAttemptAt: nil, exhausted: true)
        #expect(DetailFormat.restartAttempts(some) == "2 in the last 1 min")
        #expect(DetailFormat.restartAttempts(spent) == "Stopped restarting")
    }

    // MARK: - Connection

    @Test func anUnreadablePackageGivesOneSentenceForEverySurface() throws {
        let unreadable = try triple("package-unreadable").detail
        let text = "Stackly can’t read this provider’s package. Run stackly doctor."

        #expect(DetailFormat.connectionUnavailableText(unreadable) == text)
        #expect(DetailFormat.copyConnectionReason(unreadable) == text)
        #expect(DetailFormat.primaryConnection(unreadable) == nil)
        for name in ["postgresql", "nats", "unavailable"] {
            #expect(DetailFormat.connectionUnavailableText(try triple(name).detail) == nil)
        }
    }

    @Test func eachUnavailableReasonHasItsSentence() throws {
        let nats = try triple("nats").detail
        let retained = try triple("unavailable").detail
        #expect(
            nats.connections.map(DetailFormat.unavailableText)
                == Array(repeating: "Start the instance to assign its port.", count: 2))
        #expect(
            retained.connections.map(DetailFormat.unavailableText)
                == Array(repeating: "This instance was removed.", count: 2))
        #expect(DetailFormat.copyConnectionReason(nats) == "Start the instance to assign its port.")
        #expect(DetailFormat.copyConnectionReason(nil) == "No connection string available.")
    }

    @Test func anAvailableEntryHasNoReasonAndTheFirstOneIsThePrimary() throws {
        let detail = try triple("postgresql").detail
        #expect(detail.connections.map(DetailFormat.unavailableText) == [nil, nil])
        #expect(DetailFormat.copyConnectionReason(detail) == nil)
        #expect(DetailFormat.primaryConnection(detail)?.name == "uri")
    }

    @Test func aProviderWithoutAConnectionBlockHasNoStringToCopy() throws {
        let detail = try triple("postgresql").detail
        let bare = InstanceDetail(
            instanceId: detail.instanceId, revision: detail.revision, provider: detail.provider,
            runtime: detail.runtime, dataPath: detail.dataPath, startup: detail.startup,
            secrets: [], dependents: [], connections: [])
        #expect(DetailFormat.copyConnectionReason(bare) == "No connection string available.")
        #expect(DetailFormat.connectionUnavailableText(bare) == nil)
    }

    @Test func aStateAReasonOrAPartThisBuildDoesNotKnowAsksForAnUpdate() {
        let update = "Update Stackly to copy this connection string."
        let part = ConnectionPart(kind: .text, text: "x", secret: nil)
        func entry(
            _ state: ConnectionAvailability, _ reason: ConnectionUnavailableReason?,
            masked: String?, parts: [ConnectionPart]
        ) -> ConnectionString {
            ConnectionString(
                name: "uri", title: "URI", state: state, reason: reason, masked: masked,
                parts: parts, portMayChange: false)
        }
        #expect(
            DetailFormat.unavailableText(for: entry(.unrecognized, nil, masked: nil, parts: []))
                == update)
        #expect(
            DetailFormat.unavailableText(
                for: entry(.unavailable, .unrecognized, masked: nil, parts: [])) == update)
        #expect(
            DetailFormat.unavailableText(
                for: entry(
                    .available, nil, masked: "x",
                    parts: [ConnectionPart(kind: .unrecognized, text: nil, secret: nil)]))
                == update)
        #expect(
            DetailFormat.unavailableText(for: entry(.available, nil, masked: "x", parts: [part]))
                == nil)
    }

    @Test func theMissingReferenceOfAnEntryIsFoundInTheSecrets() throws {
        let detail = try triple("unavailable").detail
        let reference = try #require(detail.secrets.first?.reference)
        let entry = ConnectionString(
            name: "uri", title: "URI", state: .available, reason: nil, masked: "x:****",
            parts: [
                ConnectionPart(
                    kind: .secret, text: nil,
                    secret: ConnectionSecretPart(field: "superuserPassword", reference: reference))
            ], portMayChange: false)
        #expect(DetailFormat.missingReference(in: entry, secrets: detail.secrets) == reference)
        #expect(DetailFormat.missingReference(in: entry, secrets: []) == nil)
    }

}
