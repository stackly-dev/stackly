import Foundation
import StacklyClient

/// How the data size row reads: the state of the `instance.get_data_usage` call.
nonisolated public enum DataSizeState: Equatable, Sendable {
    /// The call is on its way.
    case loading
    /// The daemon answered; `usage` is `nil` when it holds no fresh result.
    case loaded(DataUsage?)
    /// The call failed.
    case unavailable
}

/// The words of the rows the detail page adds to the instance facts (spec 0021, *Format
/// bersama*). Pure functions of the DTOs, so the Summary tab, the inspector, and the tests
/// say the same thing.
nonisolated public enum DetailFormat {
    // MARK: - Connection

    /// "Stackly can’t read this provider’s package. Run stackly doctor.": the one sentence for
    /// a package that cannot be read, used by the Connection card, the Copy Connection String
    /// item, the Connection row of the inspector, and the reason of the `copyConnectionString`
    /// command, so no surface gives another cause (spec 0021, *Connection*).
    static let packageUnavailableText =
        "Stackly can’t read this provider’s package. Run stackly doctor."

    /// "This provider doesn’t describe a connection string. Use the endpoints above."
    static let noConnectionText =
        "This provider doesn’t describe a connection string. Use the endpoints above."

    /// "Update Stackly to copy this connection string.", for a state, a reason, or a part this
    /// build does not know.
    static let updateText = "Update Stackly to copy this connection string."

    /// The text that stands in for every connection string when the pinned package cannot be
    /// read, or `nil` when it can.
    /// - Parameter detail: The detail.
    public static func connectionUnavailableText(_ detail: InstanceDetail) -> String? {
        detail.provider.packageReadable ? nil : packageUnavailableText
    }

    /// Why one connection string cannot be copied, or `nil` when it can.
    /// A state, a reason, or a part this build does not know counts as unavailable, with the
    /// text that asks for an update.
    /// - Parameter connection: The entry.
    public static func unavailableText(for connection: ConnectionString) -> String? {
        switch connection.state {
        case .available:
            if connection.masked == nil
                || connection.parts.contains(where: { $0.kind == .unrecognized })
            {
                return updateText
            }
            return nil
        case .unavailable:
            switch connection.reason {
            case .portNotAssigned: return "Start the instance to assign its port."
            case .packageUnavailable: return packageUnavailableText
            case .retained: return "This instance was removed."
            case .unrecognized, nil: return updateText
            }
        case .unrecognized:
            return updateText
        }
    }

    /// The reason the Copy Connection String command cannot run, or `nil` when it can: the
    /// text of an unreadable package, "No connection string available." without an entry, or
    /// the reason of the first entry.
    /// - Parameter detail: The detail, or `nil` while it has not loaded.
    public static func copyConnectionReason(_ detail: InstanceDetail?) -> String? {
        guard let detail else { return "No connection string available." }
        if let text = connectionUnavailableText(detail) { return text }
        guard let first = detail.connections.first else {
            return "No connection string available."
        }
        return unavailableText(for: first)
    }

    /// The entry the Copy Connection String command copies: the first one, when it can be
    /// copied. `nil` when there is none, or the first one cannot be copied.
    /// - Parameter detail: The detail.
    public static func primaryConnection(_ detail: InstanceDetail) -> ConnectionString? {
        guard detail.provider.packageReadable, let first = detail.connections.first,
            unavailableText(for: first) == nil
        else { return nil }
        return first
    }

    // MARK: - Endpoints

    /// The value of an endpoint row: "{host}:{port}" (an IPv6 host in square brackets),
    /// "{host}, port assigned at start", the path of the socket, or "socket, created at
    /// start".
    /// - Parameter endpoint: The endpoint of the summary.
    public static func endpointValue(_ endpoint: EndpointStatus) -> String {
        if endpoint.transport == .unix {
            return endpoint.socketPath ?? "socket, created at start"
        }
        let host = endpoint.host ?? "—"
        guard let port = endpoint.port else { return "\(host), port assigned at start" }
        return (host.contains(":") ? "[\(host)]" : host) + ":\(port)"
    }

    /// What the Copy button of an endpoint writes: the address or the path, only when it is
    /// known. `nil` for an endpoint with no port yet or no socket yet.
    /// - Parameter endpoint: The endpoint of the summary.
    public static func endpointCopyText(_ endpoint: EndpointStatus) -> String? {
        if endpoint.transport == .unix { return endpoint.socketPath }
        guard endpoint.host != nil, endpoint.port != nil else { return nil }
        return endpointValue(endpoint)
    }

    // MARK: - Details

    /// "{name} {version} · {arch}": the name from the pinned package (the software name of the
    /// summary before the detail loaded), the exact version, and "arm64" or "x86_64".
    /// - Parameters:
    ///   - software: The software name of the summary, the fallback of the display name.
    ///   - detail: The detail, or `nil` while it has not loaded.
    ///   - runtimeVersion: The version of the summary, the fallback while the detail loads.
    public static func version(
        software: String, detail: InstanceDetail?, runtimeVersion: String
    ) -> String {
        let name = detail?.provider.displayName ?? software
        guard let runtime = detail?.runtime else { return "\(name) \(runtimeVersion)" }
        let base = "\(name) \(runtime.version)"
        switch runtime.target.arch {
        case .aarch64: return base + " · arm64"
        case .x8664: return base + " · x86_64"
        case .unrecognized: return base
        }
    }

    /// "sha256:" and the first 12 hex digits of the artifact digest, "External runtime" for a
    /// runtime Stackly did not install, or "—".
    /// - Parameter runtime: The runtime of the detail.
    public static func digest(_ runtime: DetailRuntime) -> String {
        if runtime.installMode == .external { return "External runtime" }
        guard let sha = runtime.artifactSha256 else { return "—" }
        return "sha256:" + sha.prefix(12)
    }

    /// - Parameter runtime: The runtime of the detail.
    public static func fullDigest(_ runtime: DetailRuntime) -> String? {
        runtime.artifactSha256.map { "sha256:" + $0 }
    }

    /// The provider as the Details card writes it: "{name} {revision}".
    /// - Parameter provider: The pinned provider.
    public static func provider(_ provider: DetailProvider) -> String {
        "\(provider.name) \(provider.revision)"
    }

    /// The data folder with the home folder written as `~`.
    /// - Parameters:
    ///   - path: The absolute path.
    ///   - home: The home folder of the person.
    public static func dataPath(_ path: String, home: String) -> String {
        let root = home.hasSuffix("/") ? String(home.dropLast()) : home
        guard root.isEmpty == false, path == root || path.hasPrefix(root + "/") else {
            return path
        }
        return "~" + path.dropFirst(root.count)
    }

    /// The size of the data folder: "1.2 GB", "at least 1.2 GB" for a scan that stopped at a
    /// limit, "Calculating…" while it runs, "—" when the daemon holds no result, and
    /// "Unavailable" when it failed.
    /// - Parameter state: The state of the call.
    public static func dataSize(_ state: DataSizeState) -> String {
        switch state {
        case .loading: return "Calculating…"
        case .unavailable: return "Unavailable"
        case .loaded(let usage):
            guard let usage else { return "—" }
            let size = bytes(usage.bytes)
            return usage.complete ? size : "at least " + size
        }
    }

    /// "{n} bytes" under 1 000, else kB, MB, GB, or TB with 1 000 as the step and one decimal.
    /// - Parameter count: The number of bytes.
    public static func bytes(_ count: UInt64) -> String {
        if count < 1_000 { return "\(count) bytes" }
        let units = ["kB", "MB", "GB", "TB"]
        var value = Double(count) / 1_000
        var index = 0
        // The same rule as the CLI: a value that rounds to 1000.0 moves up one unit.
        while index < units.count - 1, (value * 10).rounded(.toNearestOrAwayFromZero) >= 10_000 {
            value /= 1_000
            index += 1
        }
        return String(format: "%.1f %@", value, units[index])
    }

    /// "scanned 3m ago".
    /// - Parameters:
    ///   - usage: The measured size.
    ///   - now: The moment to count to.
    public static func scanned(_ usage: DataUsage, now: Date) -> String {
        "scanned " + RelativeAge.text(of: usage.scannedAt, now: now)
    }

    /// "Off", "At login", or "Unknown".
    /// - Parameter startup: The startup policy.
    public static func autostart(_ startup: StartupPolicyKind) -> String {
        switch startup {
        case .manual: "Off"
        case .atLogin: "At login"
        case .unrecognized: "Unknown"
        }
    }

    /// "{n} in the last {m} min", "Stopped restarting", or `nil` when nothing restarted.
    /// - Parameter restart: The restart state of the summary.
    public static func restartAttempts(_ restart: InstanceRestart) -> String? {
        if restart.exhausted { return "Stopped restarting" }
        guard restart.attempts > 0 else { return nil }
        return "\(restart.attempts) in the last \(restart.windowSeconds / 60) min"
    }

    /// The uptime of the process, or "—" without one.
    /// - Parameters:
    ///   - instance: The instance.
    ///   - now: The moment to count to.
    public static func uptime(_ instance: InstanceSummary, now: Date) -> String {
        guard let process = instance.process else { return "—" }
        return UptimeText.text(since: process.startedAt, now: now)
    }

    // MARK: - Dependents and secrets

    /// "Nothing depends on this instance."
    static let noDependentsText = "Nothing depends on this instance."

    /// "{name} · Dedicated" or "{name} · Shared", with " · lease active" when the environment
    /// holds an active lease.
    /// - Parameter dependent: The environment.
    public static func dependent(_ dependent: DetailDependent) -> String {
        let ownership =
            switch dependent.ownership {
            case .dedicated: "Dedicated"
            case .shared: "Shared"
            case .unrecognized: "Unknown"
            }
        return "\(dependent.name) · \(ownership)" + (dependent.leaseActive ? " · lease active" : "")
    }

    /// What stands next to the reference of a secret that is not known to be there: "Missing;
    /// starting asks for it." or "Status unknown", and `nil` for a secret that is present.
    /// - Parameter secret: The secret.
    public static func secretNote(_ secret: DetailSecret) -> String? {
        switch secret.status {
        case .present: nil
        case .missing: "Missing; starting asks for it."
        case .unknown, .unrecognized: "Status unknown"
        }
    }

    /// "{reference} has no stored value yet.", the reason Show Password and Copy are off.
    /// - Parameter reference: The reference without a value.
    public static func missingValueReason(_ reference: String) -> String {
        "\(reference) has no stored value yet."
    }

    /// The first reference of `connection` whose secret is `missing`, or `nil`.
    /// - Parameters:
    ///   - connection: The entry.
    ///   - secrets: The secrets of the detail.
    public static func missingReference(
        in connection: ConnectionString, secrets: [DetailSecret]
    ) -> String? {
        let missing = Set(secrets.filter { $0.status == .missing }.map(\.reference))
        return ConnectionComposer.references(in: connection.parts).first { missing.contains($0) }
    }
}
