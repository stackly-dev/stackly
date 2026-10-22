import Foundation
import StacklyClient

/// The words an instance row and the inspector use for endpoints (spec 0022, *Halaman
/// Instances*). Pure functions of the DTO.
nonisolated public enum InstanceText {
    /// The port text of a row: per endpoint in DTO order, `:{port}` for a port, `auto` for a
    /// port the daemon assigns at start, and `socket` for a Unix socket, separated by a space.
    /// An instance without endpoints gets "—".
    ///
    /// - Parameter endpoints: The endpoints of the summary.
    public static func ports(_ endpoints: [EndpointStatus]?) -> String {
        let parts = (endpoints ?? []).map(portPart)
        return parts.isEmpty ? "—" : parts.joined(separator: " ")
    }

    private static func portPart(_ endpoint: EndpointStatus) -> String {
        if endpoint.transport == .unix { return "socket" }
        return endpoint.port.map { ":\($0)" } ?? "auto"
    }

    /// The port text VoiceOver reads: "port 5432", "ports 4222 and 8222", "socket", or
    /// "automatic port". Different kinds are joined by a comma.
    ///
    /// - Parameter endpoints: The endpoints of the summary.
    public static func spokenPorts(_ endpoints: [EndpointStatus]?) -> String {
        let all = endpoints ?? []
        var numbers: [UInt16] = []
        var sockets = 0
        var automatic = 0
        for endpoint in all {
            if endpoint.transport == .unix {
                sockets += 1
            } else if let port = endpoint.port {
                numbers.append(port)
            } else {
                automatic += 1
            }
        }
        var phrases: [String] = []
        if numbers.isEmpty == false {
            phrases.append(
                (numbers.count == 1 ? "port " : "ports ") + list(numbers.map(String.init)))
        }
        if sockets > 0 { phrases.append(sockets == 1 ? "socket" : "\(sockets) sockets") }
        if automatic > 0 {
            phrases.append(automatic == 1 ? "automatic port" : "\(automatic) automatic ports")
        }
        return phrases.isEmpty ? "no endpoints" : phrases.joined(separator: ", ")
    }

    private static func list(_ items: [String]) -> String {
        switch items.count {
        case 0: ""
        case 1: items[0]
        case 2: "\(items[0]) and \(items[1])"
        default: items.dropLast().joined(separator: ", ") + ", and " + (items.last ?? "")
        }
    }

    /// The first port as a number, for sorting: `nil` for a socket or an automatic port.
    public static func firstPort(_ endpoints: [EndpointStatus]?) -> UInt16? {
        guard let first = endpoints?.first, first.transport != .unix else { return nil }
        return first.port
    }

    /// The line of one endpoint in the inspector:
    /// - `{name}  {host}:{port}` (an IPv6 host in square brackets),
    /// - `{name}  {host}, port assigned at start`,
    /// - `{name}  {socketPath}`, or
    /// - `{name}  socket, created at start`.
    public static func endpointLine(_ endpoint: EndpointStatus) -> String {
        let name = endpoint.name
        if endpoint.transport == .unix {
            return "\(name)  \(endpoint.socketPath ?? "socket, created at start")"
        }
        let host = endpoint.host ?? "—"
        let shown = host.contains(":") ? "[\(host)]" : host
        guard let port = endpoint.port else {
            return "\(name)  \(host), port assigned at start"
        }
        return "\(name)  \(shown):\(port)"
    }
}

/// How long a process has run, in the style of spec 0008: the two largest units without a
/// space, the smaller one padded (`12s`, `3m04s`, `1h02m`, `2d05h`).
nonisolated public enum UptimeText {
    /// The uptime text.
    ///
    /// - Parameters:
    ///   - startedAt: When the process started.
    ///   - now: The moment to count to; a fixed value in fixtures and tests.
    public static func text(since startedAt: Date, now: Date) -> String {
        let seconds = max(0, Int(now.timeIntervalSince(startedAt)))
        let (days, hours, minutes, secs) = (
            seconds / 86_400, (seconds / 3_600) % 24, (seconds / 60) % 60, seconds % 60
        )
        if days > 0 { return "\(days)d" + pad(hours) + "h" }
        if hours > 0 { return "\(hours)h" + pad(minutes) + "m" }
        if minutes > 0 { return "\(minutes)m" + pad(secs) + "s" }
        return "\(secs)s"
    }

    private static func pad(_ value: Int) -> String {
        value < 10 ? "0\(value)" : "\(value)"
    }
}

/// How long ago something was, in its largest whole unit: "now", "{n}s ago", "{n}m ago",
/// "{n}h ago", or "{n}d ago".
nonisolated public enum RelativeAge {
    /// The age text.
    ///
    /// - Parameters:
    ///   - date: When it happened.
    ///   - now: The moment to count to.
    public static func text(of date: Date, now: Date) -> String {
        let seconds = Int(now.timeIntervalSince(date))
        if seconds < 1 { return "now" }
        if seconds < 60 { return "\(seconds)s ago" }
        if seconds < 3_600 { return "\(seconds / 60)m ago" }
        if seconds < 86_400 { return "\(seconds / 3_600)h ago" }
        return "\(seconds / 86_400)d ago"
    }
}
