import Foundation

/// What the launch record `background-agent.json` says (spec 0015, *GUI*).
nonisolated struct LaunchRecordStatus: Sendable, Equatable {
    /// Whether the file exists and is a regular file. It is proof that launchd ran the
    /// daemon once, so that the user gave the permission.
    var exists: Bool
    /// When launchd last started the daemon, or `nil` when there is no file, it holds
    /// more than 4 KiB, or the value is not a valid RFC 3339 time.
    var lastLaunchedAt: Date?

    /// No file.
    static let missing = LaunchRecordStatus(exists: false, lastLaunchedAt: nil)
}

/// Reads the launch record, off the main actor, right after each status read.
nonisolated protocol LaunchRecordReading: Sendable {
    /// The record's status.
    @concurrent func read() async -> LaunchRecordStatus
}

/// The live ``LaunchRecordReading``: `<home>/Library/Application Support/Stackly/background-agent.json`.
nonisolated struct LaunchRecordReader: LaunchRecordReading {
    /// The most bytes of the file that are read.
    static let sizeLimit = 4096

    private let url: URL

    /// Creates a reader for the default folder under `home`.
    init(home: URL) {
        url = home.appending(path: "Library/Application Support/Stackly/background-agent.json")
    }

    @concurrent func read() async -> LaunchRecordStatus {
        Self.read(at: url)
    }

    /// The status of the record at `url`.
    static func read(at url: URL) -> LaunchRecordStatus {
        guard
            let values = try? url.resourceValues(forKeys: [.isRegularFileKey, .isSymbolicLinkKey]),
            values.isRegularFile == true, values.isSymbolicLink != true
        else {
            return .missing
        }
        guard let handle = try? FileHandle(forReadingFrom: url) else {
            return LaunchRecordStatus(exists: true, lastLaunchedAt: nil)
        }
        defer { try? handle.close() }
        guard let data = try? handle.read(upToCount: sizeLimit + 1), data.count <= sizeLimit,
            let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
            let text = object["lastLaunchedAt"] as? String
        else {
            return LaunchRecordStatus(exists: true, lastLaunchedAt: nil)
        }
        return LaunchRecordStatus(exists: true, lastLaunchedAt: parse(text))
    }

    /// An RFC 3339 time with or without fractional seconds (more than three digits are
    /// cut), or `nil`.
    static func parse(_ text: String) -> Date? {
        var normalized = text
        if let dot = text.firstIndex(of: "."),
            let end = text[dot...].firstIndex(where: { $0 == "Z" || $0 == "+" || $0 == "-" })
        {
            let digits = text[text.index(after: dot)..<end]
            guard digits.isEmpty == false, digits.allSatisfy(\.isNumber) else { return nil }
            normalized = String(text[..<dot]) + "." + digits.prefix(3) + text[end...]
        }
        let withFraction = ISO8601DateFormatter()
        withFraction.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        if let date = withFraction.date(from: normalized) { return date }
        let plain = ISO8601DateFormatter()
        plain.formatOptions = [.withInternetDateTime]
        return plain.date(from: normalized)
    }
}
