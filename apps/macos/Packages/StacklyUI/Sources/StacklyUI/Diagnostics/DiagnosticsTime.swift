import Foundation

/// The time format of the Diagnostics page: the abbreviated date and the time with seconds,
/// in the locale's style.
nonisolated public enum DiagnosticsTime {
    /// Formats `date` with `Date.FormatStyle(date: .abbreviated, time: .standard)`.
    /// Views pass the system calendar, locale, and time zone; tests pass fixed ones.
    /// - Parameters:
    ///   - date: The time to show.
    ///   - calendar: The calendar of the date.
    ///   - locale: The locale of the format.
    ///   - timeZone: The time zone of the format.
    public static func format(
        _ date: Date,
        calendar: Calendar,
        locale: Locale,
        timeZone: TimeZone
    ) -> String {
        var calendar = calendar
        calendar.timeZone = timeZone
        let style = Date.FormatStyle(
            date: .abbreviated,
            time: .standard,
            locale: locale,
            calendar: calendar,
            timeZone: timeZone
        )
        return date.formatted(style)
    }
}
