import Foundation

/// The search of the Instances page (spec 0022, *Halaman Instances*): every word of the
/// query must match one of the row's fields, ignoring case and diacritics, and each word is
/// checked on its own.
nonisolated public enum InstanceSearch {
    /// The fields a word can match: the name, the display name, the provider's display name,
    /// the provider ID, the software, the version, and every port as `:5432` and `5432`.
    static func fields(of row: InstanceRowModel) -> [String] {
        let summary = row.summary
        var fields = [
            summary.name, summary.displayName, row.providerTitle, summary.provider.name,
            summary.software, row.version,
        ].compactMap { $0 }
        for endpoint in summary.endpoints ?? [] {
            if let port = endpoint.port {
                fields.append(":\(port)")
                fields.append("\(port)")
            }
        }
        return fields
    }

    /// The words of a query: split at white space, empty ones dropped.
    public static func words(of query: String) -> [String] {
        query.split(whereSeparator: \.isWhitespace).map(String.init)
    }

    /// Whether a row matches the query. An empty query matches every row.
    /// - Parameters:
    ///   - row: The row.
    ///   - query: The text in the search field.
    public static func matches(_ row: InstanceRowModel, query: String) -> Bool {
        let words = words(of: query)
        guard words.isEmpty == false else { return true }
        let fields = fields(of: row)
        return words.allSatisfy { word in fields.contains { $0.localizedStandardContains(word) } }
    }
}
