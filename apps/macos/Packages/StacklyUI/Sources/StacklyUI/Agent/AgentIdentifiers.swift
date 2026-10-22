import Foundation

/// The identifiers of the app bundle and its background agent (spec 0015, *Identifier
/// sementara*). Temporary until OQ-001 closes; the Rust side keeps the same values in
/// `stackly-platform::bundle`, and both are tested against
/// `protocols/fixtures/bundle/identifiers.json`.
nonisolated public enum AgentIdentifiers {
    /// The bundle ID of the app.
    public static let appBundleID = "com.ojiepermana.stackly"
    /// The label of the launchd job, also the code signing identifier of `stacklyd`.
    public static let daemonLabel = "com.ojiepermana.stackly.daemon"
    /// The file name of the LaunchAgent plist.
    public static let daemonPlistFile = "com.ojiepermana.stackly.daemon.plist"
    /// The code signing identifier of `stacklyd`.
    public static let daemonSigningID = "com.ojiepermana.stackly.daemon"
    /// The code signing identifier of the command line tool.
    public static let cliSigningID = "com.ojiepermana.stackly.cli"
    /// The label prefix of the temporary agents native tests register.
    public static let testAgentLabelPrefix = "dev.stackly.test.agent."
    /// The file name of the app executable in `Contents/MacOS`.
    public static let appExecutable = "Stackly"
    /// The file name of the daemon in `Contents/MacOS`.
    public static let daemonExecutable = "stacklyd"
    /// The file name of the command line tool in `Contents/MacOS`. The command stays
    /// `stackly`; only the file in the bundle has the longer name.
    public static let cliExecutable = "stackly-cli"
    /// The folder of the executables, relative to the bundle root.
    public static let macosDir = "Contents/MacOS"
    /// The folder of the LaunchAgent plist, relative to the bundle root.
    public static let launchAgentsDir = "Contents/Library/LaunchAgents"
    /// The folder of the bundle's resources, relative to the bundle root.
    public static let resourcesDir = "Contents/Resources"
    /// The `BundleProgram` of the LaunchAgent plist.
    public static let daemonBundleProgram = "Contents/MacOS/stacklyd"
}
