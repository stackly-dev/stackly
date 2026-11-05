import Foundation

/// The live ``CommandLineToolControlling`` (spec 0015, *Instalasi CLI*): it links
/// `~/.local/bin/stackly` and the manual pages in `~/.local/share/man/man1` to the bundle
/// that runs. It is the only file of `StacklyUI` that creates, replaces, or deletes files
/// (a source rule keeps it so), and it never touches a shell file, `/etc/paths.d`, or
/// `/usr/local/bin`, and never asks for admin rights.
nonisolated final class CommandLineTool: CommandLineToolControlling {
    let home: URL
    private let bundle: URL

    /// Creates the tool for `bundle`, a canonical bundle path, with links under `home`.
    init(home: URL, bundle: URL) {
        self.home = home
        self.bundle = bundle
    }

    /// The tool for the running app: the home from the password database and the canonical
    /// path of the running bundle.
    static func live() -> CommandLineTool {
        CommandLineTool(
            home: UserHome.current(), bundle: CanonicalPath.resolve(Bundle.main.bundleURL))
    }

    // MARK: - Paths

    private var binFolder: URL { home.appending(path: ".local/bin") }
    private var manFolder: URL { home.appending(path: ".local/share/man/man1") }
    private var commandLink: URL { binFolder.appending(path: "stackly") }
    private var bundleManFolder: URL {
        bundle.appending(path: AgentIdentifiers.resourcesDir).appending(path: "man/man1")
    }

    var bundledTool: URL? {
        let url = bundle.appending(path: AgentIdentifiers.macosDir)
            .appending(path: AgentIdentifiers.cliExecutable)
        return FileManager.default.isExecutableFile(atPath: url.path) ? url : nil
    }

    /// The manual pages in the bundle, `stackly*.1`, sorted.
    private func bundleManPages() -> [String] {
        let names =
            (try? FileManager.default.contentsOfDirectory(atPath: bundleManFolder.path)) ?? []
        return names.filter { $0.hasPrefix("stackly") && $0.hasSuffix(".1") }.sorted()
    }

    // MARK: - Status

    @concurrent func status() async -> CommandLineToolStatus {
        let expected =
            bundledTool
            ?? bundle.appending(path: AgentIdentifiers.macosDir)
            .appending(path: AgentIdentifiers.cliExecutable)
        let command = state(
            of: commandLink, expectedTarget: expected, managed: Self.isManagedCommand)
        let pages = bundleManPages().map { name in
            CommandLineToolStatus.ManPage(
                name: name,
                state: state(
                    of: manFolder.appending(path: name),
                    expectedTarget: bundleManFolder.appending(path: name),
                    managed: Self.isManagedPage
                ).state)
        }
        return CommandLineToolStatus(
            command: command.state, commandTarget: command.target, manPages: pages)
    }

    /// Whether a link target is the command of some Stackly bundle.
    private static func isManagedCommand(_ target: String) -> Bool {
        target.hasSuffix(".app/\(AgentIdentifiers.macosDir)/\(AgentIdentifiers.cliExecutable)")
    }

    /// Whether a link target is a manual page of some Stackly bundle.
    private static func isManagedPage(_ target: String) -> Bool {
        let name = URL(filePath: target).lastPathComponent
        return target.contains(".app/\(AgentIdentifiers.resourcesDir)/man/man1/stackly")
            && name.hasPrefix("stackly") && name.hasSuffix(".1")
    }

    /// The state of the link at `path`, and its target when it is a link.
    private func state(
        of path: URL, expectedTarget: URL, managed: (String) -> Bool
    ) -> (state: CommandLineLinkState, target: String?) {
        let manager = FileManager.default
        guard let destination = try? manager.destinationOfSymbolicLink(atPath: path.path) else {
            // Not a link: nothing there, or something Stackly did not make.
            return (Self.exists(path) ? .conflict : .notInstalled, nil)
        }
        guard managed(destination) else { return (.conflict, destination) }
        let resolved = URL(
            fileURLWithPath: destination,
            relativeTo: path.deletingLastPathComponent()
        )
        guard manager.fileExists(atPath: resolved.path) else { return (.broken, destination) }
        let same =
            CanonicalPath.resolve(resolved).path == CanonicalPath.resolve(expectedTarget).path
        return (same ? .installed : .stale, destination)
    }

    /// Whether anything is at `path`, a dangling link included.
    private static func exists(_ path: URL) -> Bool {
        var info = stat()
        return lstat(path.path, &info) == 0
    }

    // MARK: - Install, repair, remove

    @concurrent func install() async throws(CommandLineToolError) {
        try link()
    }

    @concurrent func repair() async throws(CommandLineToolError) {
        try link()
    }

    private func link() throws(CommandLineToolError) {
        guard let tool = bundledTool else { throw .noBundledTool }
        try checkBundleLocation()
        let pages = bundleManPages()
        // Everything that can fail without writing fails first.
        try checkFolder(binFolder)
        try checkFolder(manFolder)
        let plan =
            [(commandLink, tool, Self.isManagedCommand)]
            + pages.map { name in
                (
                    manFolder.appending(path: name), bundleManFolder.appending(path: name),
                    Self.isManagedPage
                )
            }
        for (path, _, managed) in plan {
            let found = state(of: path, expectedTarget: path, managed: managed)
            if found.state == .conflict { throw .conflict(path: display(path)) }
        }
        try makeFolder(binFolder)
        try makeFolder(manFolder)
        for (path, target, _) in plan { try replace(path, with: target) }
        try removeStalePages(keeping: Set(pages))
    }

    @concurrent func remove() async throws(CommandLineToolError) {
        try checkFolder(binFolder)
        try checkFolder(manFolder)
        if Self.isLink(commandLink), Self.isManagedCommand(Self.target(of: commandLink)) {
            try delete(commandLink)
        }
        for name in Self.pageNames(in: manFolder) {
            let path = manFolder.appending(path: name)
            if Self.isLink(path), Self.isManagedPage(Self.target(of: path)) { try delete(path) }
        }
    }

    /// Deletes the managed manual page links that the bundle no longer has.
    private func removeStalePages(keeping pages: Set<String>) throws(CommandLineToolError) {
        for name in Self.pageNames(in: manFolder) where pages.contains(name) == false {
            let path = manFolder.appending(path: name)
            guard Self.isLink(path), Self.isManagedPage(Self.target(of: path)) else { continue }
            try delete(path)
        }
    }

    private static func pageNames(in folder: URL) -> [String] {
        let names = (try? FileManager.default.contentsOfDirectory(atPath: folder.path)) ?? []
        return names.filter { $0.hasPrefix("stackly") && $0.hasSuffix(".1") }.sorted()
    }

    private static func isLink(_ path: URL) -> Bool {
        (try? FileManager.default.destinationOfSymbolicLink(atPath: path.path)) != nil
    }

    private static func target(of path: URL) -> String {
        (try? FileManager.default.destinationOfSymbolicLink(atPath: path.path)) ?? ""
    }

    /// The running bundle must sit where a link to it stays valid and trusted (spec 0015,
    /// *Instalasi CLI*, audit note 149): not in an App Translocation copy, not on a read only
    /// volume such as a mounted disk image, and the bundle and every folder above it must
    /// belong to the user or to root, never be writable by others, and be writable by the
    /// group only when root owns them (so `/Applications` passes and `/Users/Shared` does
    /// not).
    private func checkBundleLocation() throws(CommandLineToolError) {
        let refused = CommandLineToolError.bundleNotInstalled(path: display(bundle))
        if bundle.pathComponents.contains("AppTranslocation") { throw refused }
        var volume = statfs()
        guard statfs(bundle.path, &volume) == 0 else { throw refused }
        if volume.f_flags & UInt32(MNT_RDONLY) != 0 { throw refused }
        var current = bundle
        while true {
            var info = stat()
            guard lstat(current.path, &info) == 0 else { throw refused }
            let owner = info.st_uid
            guard owner == getuid() || owner == 0 else { throw refused }
            if info.st_mode & mode_t(S_IWOTH) != 0 { throw refused }
            if info.st_mode & mode_t(S_IWGRP) != 0, owner != 0 { throw refused }
            let parent = current.deletingLastPathComponent()
            if parent.path == current.path || current.path == "/" { return }
            current = parent
        }
    }

    /// Every folder from below the home down to `folder` that exists (or is a link to one)
    /// must belong to the user and not be writable by the group or others, and a link must
    /// belong to the user too, so nobody else can swap a folder on the way to the links; a
    /// folder that does not exist is fine, and so is everything under it.
    private func checkFolder(_ folder: URL) throws(CommandLineToolError) {
        let root = home.pathComponents
        let parts = folder.pathComponents
        guard parts.count > root.count, Array(parts.prefix(root.count)) == root else {
            throw .folderNotSafe(path: display(folder))
        }
        var current = home
        for part in parts.dropFirst(root.count) {
            current = current.appending(path: part)
            guard try checkComponent(current) else { return }
        }
    }

    /// Checks one folder of ``checkFolder(_:)``, and says whether it exists.
    private func checkComponent(_ path: URL) throws(CommandLineToolError) -> Bool {
        var info = stat()
        guard lstat(path.path, &info) == 0 else { return false }
        let others = mode_t(S_IWGRP | S_IWOTH)
        if info.st_mode & S_IFMT == S_IFLNK {
            // A link to a folder is allowed, but the link and the folder must both be the
            // user's. A dangling link is a folder that does not exist.
            guard info.st_uid == getuid() else { throw .folderNotSafe(path: display(path)) }
            guard stat(path.path, &info) == 0 else { return false }
        }
        guard info.st_uid == getuid(), info.st_mode & others == 0 else {
            throw .folderNotSafe(path: display(path))
        }
        return true
    }

    private func makeFolder(_ folder: URL) throws(CommandLineToolError) {
        do {
            try FileManager.default.createDirectory(
                at: folder, withIntermediateDirectories: true,
                attributes: [.posixPermissions: 0o755])
        } catch {
            throw .io(path: display(folder), reason: error.localizedDescription)
        }
    }

    /// Makes `path` a link to `target` in one step: the new link is written beside it under
    /// a temporary name, then renamed over it.
    private func replace(_ path: URL, with target: URL) throws(CommandLineToolError) {
        let temporary = path.deletingLastPathComponent()
            .appending(path: ".stackly-link-\(UUID().uuidString)")
        do {
            try FileManager.default.createSymbolicLink(
                atPath: temporary.path, withDestinationPath: target.path)
        } catch {
            throw .io(path: display(path), reason: error.localizedDescription)
        }
        guard rename(temporary.path, path.path) == 0 else {
            let reason = String(cString: strerror(errno))
            try? FileManager.default.removeItem(at: temporary)
            throw .io(path: display(path), reason: reason)
        }
    }

    /// Deletes the link at `path` with `unlink(2)`, which never removes a folder, so a link
    /// swapped for a folder is never deleted with its contents.
    private func delete(_ path: URL) throws(CommandLineToolError) {
        guard unlink(path.path) == 0 || errno == ENOENT else {
            throw .io(path: display(path), reason: String(cString: strerror(errno)))
        }
    }

    private func display(_ path: URL) -> String {
        UserHome.tildePath(path.path, home: home.path)
    }
}
