import Foundation
import StacklyClient

/// The category filter of the New Instance sheet (spec 0020, *Filter kategori*).
nonisolated public enum CreateCategory: CaseIterable, Hashable, Sendable {
    /// Every runtime, also those of the testing category.
    case all
    /// Database servers.
    case databases
    /// Message brokers.
    case messaging
    /// Analytics stores.
    case analytics
    /// Caches.
    case cache
    /// Proxies.
    case proxy

    /// The label of the segment.
    public var title: String {
        switch self {
        case .all: "All"
        case .databases: "Databases"
        case .messaging: "Messaging"
        case .analytics: "Analytics"
        case .cache: "Cache"
        case .proxy: "Proxy"
        }
    }

    /// Whether a runtime of `category` shows under this filter. `testing` shows only under
    /// All, and so does a runtime the catalog does not know.
    ///
    /// - Parameter category: The category of the runtime's provider, or `nil`.
    public func includes(_ category: ProviderCategory?) -> Bool {
        switch self {
        case .all: true
        case .databases: category == .database
        case .messaging: category == .messaging
        case .analytics: category == .analytics
        case .cache: category == .cache
        case .proxy: category == .proxy
        }
    }
}

/// How a port is asked for (spec 0020, *Port*).
nonisolated public enum PortMode: Hashable, Sendable {
    /// Exactly this port, never a substitute.
    case fixed
    /// Any free port, picked when the instance starts.
    case automatic

    /// The label of the segment.
    public var title: String {
        switch self {
        case .fixed: "Fixed"
        case .automatic: "Automatic"
        }
    }
}

/// One endpoint row of the draft: an IP endpoint of the package and the port asked for.
nonisolated public struct EndpointDraft: Identifiable, Equatable, Sendable {
    /// The endpoint name from the manifest.
    public let name: String
    /// What the endpoint is for.
    public let purpose: EndpointPurpose
    /// The loopback host, shown in the title of the preflight row.
    public let host: String?
    /// Fixed or Automatic.
    public var mode: PortMode
    /// The port as typed, used by ``PortMode/fixed``.
    public var portText: String

    /// The endpoint name.
    public var id: String { name }

    /// Creates a row.
    public init(
        name: String, purpose: EndpointPurpose, host: String?, mode: PortMode, portText: String
    ) {
        self.name = name
        self.purpose = purpose
        self.host = host
        self.mode = mode
        self.portText = portText
    }

    /// The row a form starts with: Fixed with the default port of the package.
    ///
    /// - Parameter endpoint: An IP endpoint of the form.
    public init(_ endpoint: CreateEndpoint) {
        self.init(
            name: endpoint.name, purpose: endpoint.purpose, host: endpoint.host,
            mode: endpoint.defaultPort == nil ? .automatic : .fixed,
            portText: endpoint.defaultPort.map(String.init) ?? "")
    }

    /// The typed port when it is a number that fits the wire, else `nil`.
    public var port: UInt16? {
        UInt16(portText.trimmingCharacters(in: .whitespaces))
    }

    /// Whether the row asks for a Fixed port that cannot be sent.
    public var hasInvalidPort: Bool {
        mode == .fixed && port == nil
    }

    /// The request for the draft, or `nil` when a Fixed port cannot be sent (the row then
    /// shows its own message and Create stays off).
    public var request: EndpointRequest? {
        switch mode {
        case .automatic: EndpointRequest(name: name, port: .auto)
        case .fixed: port.map { EndpointRequest(name: name, port: .fixed($0)) }
        }
    }

    /// The message of a port that cannot be sent.
    public static let invalidPortMessage = "Enter a port from 1 to 65535."
}

/// How the follow up `instance.start` of "Start after creating" went.
nonisolated public enum CreateStartResult: Equatable, Sendable {
    /// The toggle was off; nothing was sent.
    case notRequested
    /// The daemon accepted the start.
    case accepted
    /// The start was refused, was not sent, or its outcome is unknown.
    case failed(detail: String?)
}

/// How a create that succeeded ended, for the window to report (spec 0020, *Selesai*).
public struct CreateOutcome: Equatable, Sendable {
    /// The name of the draft.
    public let name: String
    /// The new instance, or `nil` when the operation named none.
    public let instanceID: UUID?
    /// How the follow up start went.
    public let start: CreateStartResult

    /// Creates an outcome.
    public init(name: String, instanceID: UUID?, start: CreateStartResult) {
        self.name = name
        self.instanceID = instanceID
        self.start = start
    }

    /// What the window says after the sheet closed.
    public var feedback: Feedback {
        switch start {
        case .notRequested: .success("“\(name)” created.")
        case .accepted: .started("“\(name)” created. Starting it now.")
        case .failed(let detail):
            Feedback(
                tone: .caution, message: "“\(name)” created, but it didn’t start.", detail: detail)
        }
    }
}
