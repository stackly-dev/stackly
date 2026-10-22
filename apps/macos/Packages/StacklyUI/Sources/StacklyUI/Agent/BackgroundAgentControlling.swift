import Foundation

/// The OS side of the background agent, as the GUI sees it (spec 0015, *GUI*). The live
/// one is ``SMAppServiceAgent``, the only file that imports `ServiceManagement`; tests,
/// previews, and fixtures use ``FakeBackgroundAgentController``.
///
/// Every call runs off the main actor. `status()` never prompts and never changes the
/// registration.
nonisolated protocol BackgroundAgentControlling: Sendable {
    /// Reads the registration state.
    @concurrent func status() async throws(AgentActionError) -> AgentProbe
    /// Registers the agent; the result may need the user's approval.
    @concurrent func register() async throws(AgentActionError) -> AgentProbe
    /// Unregisters the agent.
    @concurrent func unregister() async throws(AgentActionError) -> AgentProbe
    /// Asks the OS to start the registered job now: unregister, then register.
    @concurrent func relaunch() async throws(AgentActionError) -> AgentProbe
    /// Opens Login Items in System Settings.
    func openApprovalSettings()
}

/// The canonical form of a path, the same one `std::fs::canonicalize` gives in the CLI.
nonisolated public enum CanonicalPath {
    /// `url` with every symbolic link resolved by `realpath(3)`, or `url` itself when
    /// `realpath` fails. Foundation's `resolvingSymlinksInPath()` is not used, because it
    /// drops the `/private` prefix.
    public static func resolve(_ url: URL) -> URL {
        guard let resolved = realpath(url.path, nil) else { return url }
        defer { free(resolved) }
        return URL(fileURLWithPath: String(cString: resolved))
    }
}
