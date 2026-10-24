import Foundation

/// The default data, cache, and log folders on macOS, written from a home folder the way
/// spec 0004 derives them (*Resolver macOS*). The Storage step shows them while no daemon
/// has answered yet, so the user sees where Stackly will put things before it runs.
nonisolated public enum StorageLocations {
    /// The three folders the daemon owns.
    public struct Folders: Equatable, Sendable {
        /// The data root.
        public let data: String
        /// The cache root.
        public let cache: String
        /// The log root.
        public let logs: String
    }

    /// The macOS defaults for `home`.
    /// - Parameter home: The home folder, such as `/Users/a`. A trailing slash is ignored.
    /// - Returns: `<home>/Library/Application Support/Stackly`, `<home>/Library/Caches/Stackly`,
    ///   and `<home>/Library/Logs/Stackly`.
    public static func defaults(home: String) -> Folders {
        let root = home.count > 1 && home.hasSuffix("/") ? String(home.dropLast()) : home
        return Folders(
            data: root + "/Library/Application Support/Stackly",
            cache: root + "/Library/Caches/Stackly",
            logs: root + "/Library/Logs/Stackly")
    }
}
