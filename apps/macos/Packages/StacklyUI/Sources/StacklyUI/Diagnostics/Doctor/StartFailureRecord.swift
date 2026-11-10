import Foundation

/// The record of the last daemon start that failed, which `stacklyd` writes to
/// `<data>/daemon-start-failure.json` when it exits 1 (spec 0034, AC-11).
nonisolated struct StartFailureRecord: Equatable, Sendable {
    /// When the start failed.
    var at: Date
    /// The catalog code of the error, or `daemon.start_failed`.
    var code: String
    /// The redacted error text.
    var message: String
}

/// Reads the start failure record, off the main actor.
nonisolated protocol StartFailureRecordReading: Sendable {
    /// The record, or `nil` when there is none that can be trusted.
    @concurrent func read() async -> StartFailureRecord?
}

/// The live ``StartFailureRecordReading``: the default root's
/// `<home>/Library/Application Support/Stackly/daemon-start-failure.json`, read with the rules
/// of the launch record (spec 0015) plus the ones of spec 0034: a file that is not a regular
/// file, a symlink, one another user owns, or one whose mode is looser than 0600 counts as
/// missing, and so does one over 4 KiB or one that does not parse.
nonisolated struct StartFailureRecordReader: StartFailureRecordReading {
    /// The most bytes of the file that are read.
    static let sizeLimit = 4096

    private let url: URL

    /// Creates a reader for the default folder under `home`.
    init(home: URL) {
        url = home.appending(
            path: "Library/Application Support/Stackly/daemon-start-failure.json")
    }

    @concurrent func read() async -> StartFailureRecord? {
        Self.read(at: url, owner: getuid())
    }

    /// The record at `url` when `owner` owns it, or `nil`.
    static func read(at url: URL, owner: uid_t) -> StartFailureRecord? {
        // `attributesOfItem` describes a symlink itself, never its target.
        guard let attributes = try? FileManager.default.attributesOfItem(atPath: url.path),
            attributes[.type] as? FileAttributeType == .typeRegular,
            (attributes[.ownerAccountID] as? NSNumber)?.uint32Value == owner,
            let mode = (attributes[.posixPermissions] as? NSNumber)?.uint16Value,
            mode & ~0o600 == 0
        else {
            return nil
        }
        guard let handle = try? FileHandle(forReadingFrom: url) else { return nil }
        defer { try? handle.close() }
        guard let data = try? handle.read(upToCount: sizeLimit + 1), data.count <= sizeLimit
        else {
            return nil
        }
        return parse(data)
    }

    /// The record of `data`, or `nil` when it is not one of schema 1.
    static func parse(_ data: Data) -> StartFailureRecord? {
        guard let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
            (object["schema"] as? NSNumber)?.intValue == 1,
            let text = object["at"] as? String, let at = LaunchRecordReader.parse(text),
            let code = object["code"] as? String, let message = object["message"] as? String
        else {
            return nil
        }
        return StartFailureRecord(at: at, code: code, message: message)
    }
}

/// When the daemon binary of an app bundle last changed, for the `binary_replaced` check.
nonisolated protocol BinaryChangeReading: Sendable {
    /// The modification time of `Contents/MacOS/stacklyd` in the bundle at `bundlePath`, or
    /// `nil` when it cannot be read.
    @concurrent func modifiedAt(bundlePath: String) async -> Date?
}

/// The live ``BinaryChangeReading``.
nonisolated struct BinaryChangeReader: BinaryChangeReading {
    @concurrent func modifiedAt(bundlePath: String) async -> Date? {
        let path = URL(filePath: bundlePath).appending(path: "Contents/MacOS/stacklyd").path
        return (try? FileManager.default.attributesOfItem(atPath: path))?[.modificationDate]
            as? Date
    }
}

/// What the Background Agent section adds from the start failure record and the agent's
/// binary (spec 0034, AC-11), the same facts as the client check `background_agent` of
/// `stackly doctor`.
nonisolated struct AgentStartFacts: Equatable, Sendable {
    /// `daemon_start_failed`: the record of the last failed start, or `nil`.
    var startFailure: StartFailureRecord?
    /// `binary_replaced`: the binary changed after launchd last started it.
    var binaryReplaced: Bool

    /// Nothing to add.
    static let none = AgentStartFacts(startFailure: nil, binaryReplaced: false)

    /// The facts of the section. Both need a daemon that cannot be reached. A record shows
    /// unless it is older than the daemon that ran last. `binary_replaced` needs an enabled
    /// agent, a launch record, no start failure newer than that launch, and a binary more
    /// than 2 seconds younger than the launch.
    static func resolve(
        record: StartFailureRecord?, daemonUnreachable: Bool, daemonStartedAt: Date?,
        agentEnabled: Bool, lastLaunchedAt: Date?, binaryModifiedAt: Date?
    ) -> AgentStartFacts {
        guard daemonUnreachable else { return .none }
        let shown = record.flatMap { record in
            daemonStartedAt.map { record.at >= $0 } ?? true ? record : nil
        }
        var replaced = false
        if agentEnabled, let lastLaunchedAt, let binaryModifiedAt {
            let newerFailure = record.map { $0.at > lastLaunchedAt } ?? false
            replaced = !newerFailure && binaryModifiedAt.timeIntervalSince(lastLaunchedAt) > 2
        }
        return AgentStartFacts(startFailure: shown, binaryReplaced: replaced)
    }
}

/// The readers behind ``AgentStartFacts``.
nonisolated struct StartFactsSources: Sendable {
    /// Reads the start failure record.
    let record: any StartFailureRecordReading
    /// Reads when the agent's binary changed.
    let binary: any BinaryChangeReading

    /// The readers of the default root under `home` and of the agent's bundle.
    static func live(home: URL) -> StartFactsSources {
        StartFactsSources(
            record: StartFailureRecordReader(home: home), binary: BinaryChangeReader())
    }
}
