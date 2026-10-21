import Foundation

/// Writes raw records to `records/<id>.json` in a run folder.
package enum RecordFile {
    /// The name of the raw record file of `id`, with a lowercase UUID like the
    /// Rust guard writes.
    package static func name(of id: UUID) -> String {
        "\(id.uuidString.lowercased()).json"
    }

    /// Writes `record` atomically: to `<id>.json.tmp` in the same folder
    /// first, then renamed, so the wrapper never reads a half written file.
    package static func write(_ record: Record, into runDirectory: URL) throws {
        let records = runDirectory.appending(path: "records", directoryHint: .isDirectory)
        try FileManager.default.createDirectory(at: records, withIntermediateDirectories: true)
        let file = records.appending(path: name(of: record.id))
        let temporary = records.appending(path: name(of: record.id) + ".tmp")
        try record.jsonData().write(to: temporary)
        let handle = try FileHandle(forWritingTo: temporary)
        try handle.synchronize()
        try handle.close()
        guard rename(temporary.path, file.path) == 0 else {
            throw POSIXError(POSIXErrorCode(rawValue: errno) ?? .EIO)
        }
    }
}
