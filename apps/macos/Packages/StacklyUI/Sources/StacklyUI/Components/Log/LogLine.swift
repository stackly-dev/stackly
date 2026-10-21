/// How serious a log line is: ordinary, a warning, or an error.
nonisolated public enum LogLevel: Equatable, Sendable {
    /// An ordinary line: primary text and no symbol.
    case normal
    /// A warning: caution text and the `exclamationmark.triangle.fill` symbol.
    case warning
    /// An error: critical text and the `xmark.circle.fill` symbol.
    case error

    /// The tone of the level, or `nil` for an ordinary line.
    public var tone: Tone? {
        switch self {
        case .normal: nil
        case .warning: .caution
        case .error: .critical
        }
    }

    /// The SF Symbol of the level, or `nil` for an ordinary line. It is always drawn, not
    /// only while Differentiate Without Color is on.
    public var symbol: String? {
        switch self {
        case .normal: nil
        case .warning: "exclamationmark.triangle.fill"
        case .error: "xmark.circle.fill"
        }
    }

    /// The word VoiceOver reads for the level, or `nil` for an ordinary line.
    public var word: String? {
        switch self {
        case .normal: nil
        case .warning: "Warning"
        case .error: "Error"
        }
    }
}

/// One line of a ``LogView``.
///
/// Feature 27 fills it from the log stream and decides the time format and the level.
nonisolated public struct LogLine: Identifiable, Equatable, Sendable {
    /// A stable key; the view follows and scrolls by it.
    public var id: String
    /// The time, already formatted, or `nil`.
    public var timeText: String?
    /// Where the line came from, such as `stderr`, or `nil`.
    public var source: String?
    /// The text of the line.
    public var text: String
    /// How serious the line is.
    public var level: LogLevel

    /// Creates a line.
    ///
    /// - Parameters:
    ///   - id: A stable key.
    ///   - timeText: The formatted time, or `nil`.
    ///   - source: The source, or `nil`.
    ///   - text: The text.
    ///   - level: The level; ordinary by default.
    public init(
        id: String, timeText: String? = nil, source: String? = nil, text: String,
        level: LogLevel = .normal
    ) {
        self.id = id
        self.timeText = timeText
        self.source = source
        self.text = text
        self.level = level
    }

    /// What VoiceOver reads for the line: "{time}, {source}, {Error | Warning}: {text}".
    /// An empty part and the ordinary level are left out, and a line with no part before
    /// the text reads as the text alone.
    public var accessibilityLabel: String {
        let prefix = [timeText, source, level.word]
            .compactMap { $0 }
            .filter { !$0.isEmpty }
        guard !prefix.isEmpty else { return text }
        return "\(prefix.joined(separator: ", ")): \(text)"
    }
}
