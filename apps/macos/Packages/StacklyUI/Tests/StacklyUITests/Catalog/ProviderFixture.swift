import Foundation
import StacklyClient

@testable import StacklyUI

/// The fixtures of `protocols/fixtures/provider/` (spec 0037), the files the Rust tests and
/// the CLI golden read too, and variants built from them by changing JSON keys.
enum ProviderFixture {
    /// The approval statement, one line without its line break.
    static func approvalStatement() throws -> String {
        let url = SourceTree.url(of: "protocols/fixtures/provider/approval-statement.txt")
        let text = try String(contentsOf: url, encoding: .utf8)
        return text.trimmingCharacters(in: .newlines)
    }

    /// The `ImportPreview` fixture `import-preview-<name>`.
    static func preview(_ name: String, change: ((inout [String: Any]) -> Void)? = nil) throws
        -> ImportPreview
    {
        try read("import-preview-\(name)", change: change)
    }

    /// The `ProviderDetail` fixture `detail-local-<name>`.
    static func detail(_ name: String, change: ((inout [String: Any]) -> Void)? = nil) throws
        -> ProviderDetail
    {
        try read("detail-local-\(name)", change: change)
    }

    /// The pending detail changed into one whose bytes differ from an approval of revision
    /// 0.9.0, with the changes the daemon lists.
    static func changedDetail() throws -> ProviderDetail {
        try detail("pending") { json in
            var local = json["local"] as? [String: Any] ?? [:]
            local["decision"] = [
                "decision": "approved", "decidedAt": "2026-11-01T09:00:00.000Z",
                "providerRevision": "0.9.0",
                "packageSha256": String(repeating: "9", count: 64),
            ]
            local["changes"] = ["revision", "downloads", "download_hosts"]
            json["local"] = local
        }
    }

    /// The approved detail with a revocation by the installed catalog. As `stacklyd` answers
    /// it, the entry is blocked and no version installs: every `versions[].install` is
    /// `installable: false` with reason `trust_revoked`, as in `detail-local-revoked`.
    static func catalogRevokedDetail() throws -> ProviderDetail {
        try detail("approved") { json in
            var summary = json["summary"] as? [String: Any] ?? [:]
            summary["state"] = "blocked"
            summary["stateReason"] = "trust_revoked"
            json["summary"] = summary
            json["versions"] = (json["versions"] as? [[String: Any]] ?? []).map { version in
                var version = version
                version["install"] = ["installable": false, "reason": "trust_revoked"]
                return version
            }
            json["revocation"] = [
                "by": "catalog", "since": "2026-11-05T00:00:00.000Z",
                "reason": "Starts with an insecure default; use revision 1.0.1 or later.",
                "catalogRevision": 7,
            ]
        }
    }

    private static func read<T: Decodable>(
        _ name: String, change: ((inout [String: Any]) -> Void)?
    ) throws -> T {
        let url = SourceTree.url(of: "protocols/fixtures/provider/\(name).json")
        var data = try Data(contentsOf: url)
        if let change {
            guard var json = try JSONSerialization.jsonObject(with: data) as? [String: Any] else {
                throw ContractFixture.Mismatch(description: "\(name) is not a JSON object")
            }
            change(&json)
            data = try JSONSerialization.data(withJSONObject: json)
        }
        return try StacklyJSON.decoder().decode(T.self, from: data)
    }
}

/// A folder panel that answers with a fixed path, or `nil` for Cancel.
struct FixedFolderPicker: FolderPicking {
    let path: String?

    func chooseFolder(message: String, startingAt: String?) async -> String? {
        path
    }
}
