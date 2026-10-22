import Foundation

/// The user's home folder and how a path under it is written (spec 0015, *GUI*).
nonisolated public enum UserHome {
    /// The home folder from the password database (`getpwuid_r(getuid())`), never the
    /// `HOME` variable, like spec 0004. Falls back to `NSHomeDirectory()` when the lookup
    /// fails.
    public static func current() -> URL {
        var entry = passwd()
        var result: UnsafeMutablePointer<passwd>?
        var buffer = [CChar](repeating: 0, count: 4096)
        let status = getpwuid_r(getuid(), &entry, &buffer, buffer.count, &result)
        guard status == 0, result != nil, let directory = entry.pw_dir else {
            return URL(fileURLWithPath: NSHomeDirectory())
        }
        return URL(fileURLWithPath: String(cString: directory))
    }

    /// `path` with `home` written as `~`, only on a component boundary: the home itself
    /// becomes `~`, a path under it `~/…`, and any other path (`/Users/ab/x` for the home
    /// `/Users/a`) stays whole.
    public static func tildePath(_ path: String, home: String) -> String {
        let root = home.count > 1 && home.hasSuffix("/") ? String(home.dropLast()) : home
        if path == root { return "~" }
        if root.isEmpty == false, path.hasPrefix(root + "/") {
            return "~/" + path.dropFirst(root.count + 1)
        }
        return path
    }
}
