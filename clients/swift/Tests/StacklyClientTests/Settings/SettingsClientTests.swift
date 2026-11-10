import Foundation
import StacklyClient
import Synchronization
import Testing

/// The settings and storage methods of `StacklyClient` over a fake `send` (spec 0032
/// AC-16, AC-19): reads carry no key, writes carry the revision the screen showed and the
/// key of the action, and a reply of another kind is a broken contract.
struct SettingsClientTests {
    /// Records each call with its key and answers with a fixed result.
    final class Fake: Sendable {
        private let calls = Mutex<[(call: Call, key: String?)]>([])
        private let answer: Result<Reply, ClientError>

        init(_ answer: Result<Reply, ClientError>) {
            self.answer = answer
        }

        var made: [(call: Call, key: String?)] { calls.withLock { $0 } }

        var client: SettingsClient {
            SettingsClient { [self] call, key, _ throws(ClientError) in
                calls.withLock { $0.append((call, key)) }
                return try answer.get()
            }
        }
    }

    static func location(_ kind: LocationKind, path: String = "/Users/me/Stackly")
        -> StorageLocationView
    {
        StorageLocationView(
            id: UUID(uuidString: "00000000-0000-7000-8000-\(kind.rawValue.utf8.count)00000000000")
                ?? UUID(),
            kind: kind, path: path, builtIn: true, isDefault: true,
            instanceCount: 0, reachable: true)
    }

    static func settings(revision: UInt64 = 3) -> DaemonSettingsView {
        DaemonSettingsView(
            revision: revision, defaultStartup: .manual, updateChannel: .stable, telemetry: false,
            telemetryCopyVersion: 1,
            locations: DefaultLocations(
                data: location(.data), logs: location(.logs), cache: location(.cache)),
            logRetention: LogRetention(segmentsPerStream: 8, segmentMaxBytes: 1_048_576),
            auditRetention: nil, updatedAt: Date(timeIntervalSince1970: 1_700_000_000))
    }

    @Test func getSendsNoKeyAndReadsTheSettings() async throws {
        let fake = Fake(.success(.settingsGet(Self.settings())))
        let view = try await fake.client.get()

        let made = try #require(fake.made.first)
        #expect(made.call == .settingsGet)
        #expect(made.key == nil)
        #expect(view == Self.settings())
    }

    @Test func updateSendsTheRevisionThePatchAndTheKey() async throws {
        let changed = Self.settings(revision: 4)
        let fake = Fake(.success(.settingsUpdate(changed)))
        let patch = SettingsPatch(
            defaultStartup: .atLogin, updateChannel: nil, telemetry: nil, auditRetentionDays: nil)
        let view = try await fake.client.update(
            expectedRevision: 3, patch: patch, idempotencyKey: "key-1")

        let made = try #require(fake.made.first)
        #expect(
            made.call == .settingsUpdate(UpdateSettingsParams(expectedRevision: 3, patch: patch)))
        #expect(made.key == "key-1")
        #expect(view == changed)
    }

    @Test func locationsReadsEveryLocation() async throws {
        let items = [Self.location(.data), Self.location(.cache)]
        let fake = Fake(.success(.storageListLocations(LocationList(items: items))))
        let read = try await fake.client.locations()

        #expect(try #require(fake.made.first).call == .storageListLocations)
        #expect(read == items)
    }

    @Test func setLocationSendsTheKindTheTargetTheRevisionAndTheKey() async throws {
        let moved = Self.location(.data, path: "/Volumes/Work/Stackly")
        let fake = Fake(.success(.storageSetLocation(moved)))
        let view = try await fake.client.setLocation(
            kind: .data, target: .path("/Volumes/Work"), expectedRevision: 5,
            idempotencyKey: "key-2")

        let made = try #require(fake.made.first)
        #expect(
            made.call
                == .storageSetLocation(
                    SetLocationParams(
                        kind: .data, target: .path("/Volumes/Work"), expectedRevision: 5)))
        #expect(made.key == "key-2")
        #expect(view == moved)
    }

    @Test func aReplyOfAnotherKindIsABrokenContract() async {
        let fake = Fake(.success(.settingsGet(Self.settings())))
        await #expect(throws: ClientError.self) {
            _ = try await fake.client.locations()
        }
    }

    @Test func aRefusalPassesThroughUnchanged() async throws {
        let body = ErrorBody(
            code: "resource.revision_mismatch", category: .conflict, message: "changed",
            retryable: false, resource: nil, fields: nil, hint: nil, details: nil,
            operationId: nil, rule: nil, items: nil)
        let fake = Fake(.failure(.rejected(body)))
        do {
            _ = try await fake.client.update(
                expectedRevision: 1,
                patch: SettingsPatch(
                    defaultStartup: nil, updateChannel: nil, telemetry: true,
                    auditRetentionDays: nil),
                idempotencyKey: "k")
            Issue.record("the refusal should throw")
        } catch {
            #expect(error == .rejected(body))
        }
    }
}
