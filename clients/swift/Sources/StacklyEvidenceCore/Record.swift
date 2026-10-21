import Foundation

/// Whether a test needs only fakes and fixtures, or a real provider runtime
/// or something only a macOS 27 Mac has.
///
/// `EvidenceKind` in `StacklyTestSupport` is this type.
public enum EvidenceKind: String, Codable, Sendable {
    /// Fakes, mocks, dummy servers, fake providers, or contract fixtures.
    case fixture
    /// A real provider binary, or something only a macOS 27 Mac has. Only
    /// test targets whose name ends with `NativeTests` use it.
    case native
}

/// Where a provider or runtime under test came from.
///
/// `EvidenceSource` in `StacklyTestSupport` is this type.
public enum EvidenceSource: String, Codable, Sendable {
    /// A runtime Stackly installs and owns.
    case managed
    /// A runtime found on the system, such as a Homebrew one.
    case external
    /// A fake or fixture. A native test never uses it.
    case fixture
}

/// The status of a record (PRD section 28).
package enum Status: String, Codable, Sendable {
    case notRun = "NOT_RUN"
    case pass = "PASS"
    case fail = "FAIL"
    case blocked = "BLOCKED"
    case deferred = "DEFERRED"
}

/// Whether a record comes from a test run or from a reviewer.
package enum Method: String, Codable, Sendable {
    case automated
    case manual
}

/// The language a test is written in, or `manual` for a review.
package enum Language: String, Codable, Sendable {
    case rust
    case swift
    case manual
}

/// The identity of a test, `(language, package, target, name)`, plus where it
/// lives. A Swift test has no package.
package struct TestIdentity: Codable, Equatable, Sendable {
    package var language: Language
    package var package: String?
    package var target: String?
    package var name: String
    package var file: String?
    package var line: Int?

    /// Creates the identity of a test.
    package init(
        language: Language, package: String?, target: String?, name: String, file: String?,
        line: Int?
    ) {
        self.language = language
        self.package = package
        self.target = target
        self.name = name
        self.file = file
        self.line = line
    }

    /// Writes every field, with `null` for a missing value, the same shape the
    /// Rust writer produces.
    package func encode(to encoder: any Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        try container.encode(language, forKey: .language)
        try container.encode(package, forKey: .package)
        try container.encode(target, forKey: .target)
        try container.encode(name, forKey: .name)
        try container.encode(file, forKey: .file)
        try container.encode(line, forKey: .line)
    }
}

/// One provider or runtime a test ran against.
package struct Subject: Codable, Equatable, Sendable {
    package var provider: String
    package var version: String
    package var digest: String?
    package var source: EvidenceSource

    /// Creates a subject.
    package init(provider: String, version: String, digest: String?, source: EvidenceSource) {
        self.provider = provider
        self.version = version
        self.digest = digest
        self.source = source
    }

    /// Writes every field, with `null` for a missing digest.
    package func encode(to encoder: any Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        try container.encode(provider, forKey: .provider)
        try container.encode(version, forKey: .version)
        try container.encode(digest, forKey: .digest)
        try container.encode(source, forKey: .source)
    }
}

/// One `stackly.evidence/v1` record: one test function in one run.
///
/// The Rust types in `stackly-evidence` are the source of truth, and the
/// shared examples in `schemas/evidence/examples/` keep this hand written
/// `Codable` in step with them. Unknown fields, such as the `context` of a
/// report record, are ignored when decoding.
package struct Record: Codable, Equatable, Sendable {
    /// The `schema` value of every record.
    package static let schemaName = "stackly.evidence/v1"
    /// `actual` of a Swift `PASS` when the test set no observed value.
    package static let defaultActual = "all expectations passed"
    /// `evidencePath` of an automated Swift record, relative to the run folder.
    package static let outputLog = "output.log"

    package var schema: String
    package var id: UUID
    package var covers: [String]
    package var kind: EvidenceKind
    package var method: Method
    package var test: TestIdentity
    package var status: Status
    package var expected: String
    package var actual: String?
    package var reason: String?
    package var owner: String?
    package var subjects: [Subject]
    package var steps: [String]?
    package var reviewer: String?
    package var startedAt: Date
    package var finishedAt: Date?
    package var evidencePath: String

    /// Creates a record from every field.
    package init(
        schema: String, id: UUID, covers: [String], kind: EvidenceKind, method: Method,
        test: TestIdentity, status: Status, expected: String, actual: String?, reason: String?,
        owner: String?, subjects: [Subject], steps: [String]?, reviewer: String?,
        startedAt: Date, finishedAt: Date?, evidencePath: String
    ) {
        self.schema = schema
        self.id = id
        self.covers = covers
        self.kind = kind
        self.method = method
        self.test = test
        self.status = status
        self.expected = expected
        self.actual = actual
        self.reason = reason
        self.owner = owner
        self.subjects = subjects
        self.steps = steps
        self.reviewer = reviewer
        self.startedAt = startedAt
        self.finishedAt = finishedAt
        self.evidencePath = evidencePath
    }

    private enum CodingKeys: String, CodingKey {
        case schema, id, covers, kind, method, test, status, expected, actual, reason, owner
        case subjects, steps, reviewer, startedAt, finishedAt, evidencePath
    }
}

extension Record {
    /// Reads a record written by Rust, Swift, or a reviewer. Missing optional
    /// fields read as null and missing `subjects` as an empty list, like the
    /// serde defaults on the Rust side.
    package init(from decoder: any Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        schema = try container.decode(String.self, forKey: .schema)
        id = try container.decode(UUID.self, forKey: .id)
        covers = try container.decode([String].self, forKey: .covers)
        kind = try container.decode(EvidenceKind.self, forKey: .kind)
        method = try container.decode(Method.self, forKey: .method)
        test = try container.decode(TestIdentity.self, forKey: .test)
        status = try container.decode(Status.self, forKey: .status)
        expected = try container.decode(String.self, forKey: .expected)
        actual = try container.decodeIfPresent(String.self, forKey: .actual)
        reason = try container.decodeIfPresent(String.self, forKey: .reason)
        owner = try container.decodeIfPresent(String.self, forKey: .owner)
        subjects = try container.decodeIfPresent([Subject].self, forKey: .subjects) ?? []
        steps = try container.decodeIfPresent([String].self, forKey: .steps)
        reviewer = try container.decodeIfPresent(String.self, forKey: .reviewer)
        startedAt = try RFC3339.decode(forKey: .startedAt, in: container)
        finishedAt = try RFC3339.decodeIfPresent(forKey: .finishedAt, in: container)
        evidencePath = try container.decode(String.self, forKey: .evidencePath)
    }

    /// Writes every field in camelCase, with `null` for a missing value, a
    /// lowercase UUID like Rust writes, and RFC 3339 UTC timestamps.
    package func encode(to encoder: any Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        try container.encode(schema, forKey: .schema)
        try container.encode(id.uuidString.lowercased(), forKey: .id)
        try container.encode(covers, forKey: .covers)
        try container.encode(kind, forKey: .kind)
        try container.encode(method, forKey: .method)
        try container.encode(test, forKey: .test)
        try container.encode(status, forKey: .status)
        try container.encode(expected, forKey: .expected)
        try container.encode(actual, forKey: .actual)
        try container.encode(reason, forKey: .reason)
        try container.encode(owner, forKey: .owner)
        try container.encode(subjects, forKey: .subjects)
        try container.encode(steps, forKey: .steps)
        try container.encode(reviewer, forKey: .reviewer)
        try container.encode(RFC3339.string(from: startedAt), forKey: .startedAt)
        try container.encode(finishedAt.map(RFC3339.string(from:)), forKey: .finishedAt)
        try container.encode(evidencePath, forKey: .evidencePath)
    }

    /// Encodes the record as pretty JSON with a final newline, the way it is
    /// written to `records/<id>.json`.
    package func jsonData() throws -> Data {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .withoutEscapingSlashes]
        var data = try encoder.encode(self)
        data.append(0x0A)
        return data
    }
}

/// RFC 3339 timestamps in UTC, such as `2026-10-18T08:21:40.112Z`.
package enum RFC3339 {
    private static let parser = Date.ISO8601FormatStyle(includingFractionalSeconds: true)
    private static let wholeSeconds = Date.ISO8601FormatStyle()

    /// Formats `date` in UTC, rounded to the nearest millisecond.
    ///
    /// `ISO8601FormatStyle` truncates the fraction, and a parsed `.112` is
    /// stored as slightly less than that, so it would come back as `.111`.
    package static func string(from date: Date) -> String {
        let milliseconds = (date.timeIntervalSince1970 * 1000).rounded()
        let seconds = (milliseconds / 1000).rounded(.down)
        let fraction = Int(milliseconds - seconds * 1000)
        let whole = wholeSeconds.format(Date(timeIntervalSince1970: seconds))
        return "\(whole.dropLast()).\(String(1000 + fraction).dropFirst())Z"
    }

    /// Parses a UTC timestamp with or without fractional seconds.
    package static func date(from text: String) -> Date? {
        try? parser.parse(text)
    }

    /// Decodes the timestamp stored under `key`.
    package static func decode<Key: CodingKey>(
        forKey key: Key, in container: KeyedDecodingContainer<Key>
    ) throws -> Date {
        let text = try container.decode(String.self, forKey: key)
        guard let date = date(from: text) else {
            throw DecodingError.dataCorruptedError(
                forKey: key, in: container,
                debugDescription: "`\(text)` is not an RFC 3339 UTC timestamp")
        }
        return date
    }

    /// Decodes the timestamp stored under `key`, or nil when it is null or
    /// missing.
    package static func decodeIfPresent<Key: CodingKey>(
        forKey key: Key, in container: KeyedDecodingContainer<Key>
    ) throws -> Date? {
        guard container.contains(key), try !container.decodeNil(forKey: key) else {
            return nil
        }
        return try decode(forKey: key, in: container)
    }
}

extension UUID {
    /// A version 7 UUID: the Unix time in milliseconds in the first 48 bits,
    /// then random bits, so IDs sort by creation time like the Rust ones.
    package static func version7(at date: Date = Date()) -> UUID {
        let milliseconds = UInt64(max(0, date.timeIntervalSince1970 * 1000))
        // Start from a random version 4 UUID: its variant bits are already
        // the RFC 9562 ones, and its other bits stay random.
        var bytes = UUID().uuid
        bytes.0 = UInt8(truncatingIfNeeded: milliseconds >> 40)
        bytes.1 = UInt8(truncatingIfNeeded: milliseconds >> 32)
        bytes.2 = UInt8(truncatingIfNeeded: milliseconds >> 24)
        bytes.3 = UInt8(truncatingIfNeeded: milliseconds >> 16)
        bytes.4 = UInt8(truncatingIfNeeded: milliseconds >> 8)
        bytes.5 = UInt8(truncatingIfNeeded: milliseconds)
        bytes.6 = (bytes.6 & 0x0F) | 0x70
        return UUID(uuid: bytes)
    }
}
