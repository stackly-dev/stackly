import Foundation

/// A daemon frame after the staged decode.
public enum DecodedFrame: Equatable, Sendable {
    /// The reply to one request.
    case response(Response)
    /// The error for one request, or for the connection.
    case error(ErrorFrame)
    /// An event of a kind this client knows.
    case event(Event)
    /// An event of a kind this client does not know. Its sequence still
    /// advances the cursor.
    case skipped(subscriptionId: UInt32, sequence: UInt64, kind: String)
    /// Log entries of one follow (spec 0027). Entries of a kind this client
    /// does not know are left out.
    case log(LogFrame)
}

/// Reads one daemon frame in the stages of Rust's `decode_server_frame`
/// (spec 0006): the JSON depth, the frame's `type`, the event kind through
/// `EventProbe`, then the full decode.
public enum ServerFrameDecoder {
    /// The frame types a daemon sends.
    static let frameTypes: Set<String> = ["response", "error", "event", "log"]

    /// Decodes `payload`.
    ///
    /// - Parameters:
    ///   - payload: One frame payload.
    ///   - maxDepth: The deepest JSON nesting accepted.
    /// - Throws: ``ClientError/outcomeUnknown(_:)`` with
    ///   `ipc.protocol_violation` for a frame nested too deep, a foreign frame
    ///   type, or a known frame that does not decode. The connection closes.
    public static func decode(_ payload: [UInt8], maxDepth: UInt32) throws(ClientError)
        -> DecodedFrame
    {
        guard JSONDepth.exceeds(payload, maxDepth) == false else { throw violation() }
        let data = Data(payload)
        let decoder = StacklyJSON.decoder()
        guard let probe = try? decoder.decode(FrameProbe.self, from: data),
            frameTypes.contains(probe.type)
        else { throw violation() }
        if probe.type == "event" {
            guard let event = try? decoder.decode(EventFrameProbe.self, from: data) else {
                throw violation()
            }
            let kind = event.content.kind.type
            guard ContractNames.eventKinds.contains(kind) else {
                return .skipped(
                    subscriptionId: event.content.subscriptionId,
                    sequence: event.content.sequence,
                    kind: kind
                )
            }
        }
        if probe.type == "log" {
            guard let log = try? decoder.decode(LogFrameProbe.self, from: data) else {
                throw violation()
            }
            return .log(log.content.frame)
        }
        guard let frame = try? decoder.decode(ServerFrame.self, from: data) else {
            throw violation()
        }
        switch frame {
        case .response(let response): return .response(response)
        case .error(let error): return .error(error)
        case .event(let event): return .event(event)
        case .log(let log): return .log(log)
        }
    }

    /// A `log` frame whose entries are decoded one by one, so an entry of a
    /// kind a newer daemon added is dropped instead of failing the frame.
    private struct LogFrameProbe: Decodable {
        let content: LenientLogFrame
    }

    private struct LenientLogFrame: Decodable {
        let subscriptionId: UInt32
        let storeId: UUID?
        let entries: [LenientEntry]?
        let gap: LogGap?
        let ended: LogFollowEnd?

        var frame: LogFrame {
            LogFrame(
                subscriptionId: subscriptionId,
                storeId: storeId,
                entries: entries?.compactMap(\.entry),
                gap: gap,
                ended: ended
            )
        }
    }

    private struct LenientEntry: Decodable {
        let entry: LogEntry?

        init(from decoder: Decoder) throws {
            entry = try? LogEntry(from: decoder)
        }
    }

    private static func violation() -> ClientError {
        .outcomeUnknown(ClientErrorCatalog.protocolViolation())
    }

    /// The frame's `type` only.
    private struct FrameProbe: Decodable {
        let type: String
    }

    /// An event frame read as far as its `EventProbe`.
    private struct EventFrameProbe: Decodable {
        let content: EventProbe
    }
}
