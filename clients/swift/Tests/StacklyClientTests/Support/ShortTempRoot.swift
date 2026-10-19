import Darwin
import Foundation
import StacklyClient

/// A private 0700 folder under `/private/tmp` with a short path, so socket
/// paths fit `sun_path`. It is removed when the test ends.
final class ShortTempRoot: Sendable {
    /// The folder's path.
    let path: String

    /// Creates the folder.
    init() throws {
        let name = "/private/tmp/stk-\(UUID().uuidString.prefix(8).lowercased())"
        guard mkdir(name, 0o700) == 0 else {
            throw POSIXError(POSIXErrorCode(rawValue: errno) ?? .EIO)
        }
        path = name
    }

    deinit {
        try? FileManager.default.removeItem(atPath: path)
    }

    /// A path inside the folder.
    func file(_ name: String) -> String {
        "\(path)/\(name)"
    }

    /// The endpoint of `stacklyd.sock` in the folder, as the client computes
    /// it for a runtime root.
    var endpoint: SocketEndpoint {
        SocketEndpoint(runtimeRootBytes: Array(path.utf8), rootOverride: true)
    }
}
