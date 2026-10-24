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

    /// The SF Symbol of the level, or `nil` for an ordinary line.
    public var symbol: String? {
        switch self {
        case .normal: nil
        case .warning: "exclamationmark.triangle.fill"
        case .error: "xmark.circle.fill"
        }
    }

}

/// One line of a ``LogView``.
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

}
