import Foundation

/// The JSON coders for the IPC contract (spec 0006).
///
/// Wire times are RFC 3339 in UTC with exactly three fractional digits, such
/// as `2026-10-18T03:04:05.123Z`. Decoding also accepts any offset and a time
/// without fractional seconds. Every contract type goes through these coders,
/// never a plain `JSONDecoder` or `JSONEncoder`.
public enum StacklyJSON {
    /// A decoder for daemon frames and every other contract type.
    public static func decoder() -> JSONDecoder {
        let decoder = JSONDecoder()
        decoder.dateDecodingStrategy = .custom { decoder in
            let container = try decoder.singleValueContainer()
            let text = try container.decode(String.self)
            guard let date = date(from: text) else {
                throw DecodingError.dataCorruptedError(
                    in: container,
                    debugDescription: "time is not an RFC 3339 instant with an offset")
            }
            return date
        }
        return decoder
    }

    /// An encoder for requests and every other contract type.
    public static func encoder() -> JSONEncoder {
        let encoder = JSONEncoder()
        encoder.dateEncodingStrategy = .custom { date, encoder in
            var container = encoder.singleValueContainer()
            try container.encode(string(from: date))
        }
        return encoder
    }

    /// The wire text of `date`, `yyyy-MM-dd'T'HH:mm:ss.SSS'Z'` in UTC.
    ///
    /// The date is rounded to the nearest millisecond, which gives back every
    /// time read from the wire exactly despite `Double` rounding.
    public static func string(from date: Date) -> String {
        let milliseconds = Int64((date.timeIntervalSince1970 * 1000).rounded())
        var (seconds, fraction) = milliseconds.quotientAndRemainder(dividingBy: 1000)
        if fraction < 0 {
            seconds -= 1
            fraction += 1000
        }
        let whole = Date(timeIntervalSince1970: TimeInterval(seconds))
            .formatted(Date.ISO8601FormatStyle(timeZone: .gmt))
        let digits = String(fraction)
        let padded = String(repeating: "0", count: 3 - digits.count) + digits
        return "\(whole.dropLast()).\(padded)Z"
    }

    /// The date of an RFC 3339 instant with an offset, with or without
    /// fractional seconds, or `nil` when `text` is not one.
    public static func date(from text: String) -> Date? {
        if let date = try? Date.ISO8601FormatStyle(includingFractionalSeconds: true).parse(text) {
            return date
        }
        return try? Date.ISO8601FormatStyle().parse(text)
    }
}
