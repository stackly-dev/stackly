import Foundation

/// Why loaded content may be out of date: a failed refresh, or data the daemon reports as
/// outdated. ``StateView`` shows it as a caution banner above the content, which is never
/// dimmed.
///
/// A daemon that cannot be reached is not a stale note: the app shell shows one banner for
/// the whole window and sets ``MutationAvailability``.
nonisolated public struct StaleNote: Hashable, Sendable {
    /// Why the content may be out of date.
    nonisolated public enum Reason: Hashable, Sendable {
        /// The last refresh failed; the content is from the last successful load.
        case refreshFailed(FailureContent)
        /// The source reports the data as out of date, such as the catalog.
        case outdated(source: String)
    }

    /// When the content was last known to be current.
    public var since: Date
    /// Why the content may be out of date.
    public var reason: Reason

    /// Creates a stale note.
    ///
    /// - Parameters:
    ///   - since: When the content was last known to be current.
    ///   - reason: Why the content may be out of date.
    public init(since: Date, reason: Reason) {
        self.since = since
        self.reason = reason
    }

    /// The stale note for a view model's inputs: a failed refresh when there is an error
    /// next to loaded data (it wins over `outdated`), else the outdated note, else `nil`.
    ///
    /// Call it only when data has loaded.
    init?(error: FailureContent?, lastLoadedAt: Date?, outdated: (since: Date, source: String)?) {
        if let error {
            assert(lastLoadedAt != nil, "a view model sets lastLoadedAt together with value")
            self.init(since: lastLoadedAt ?? .now, reason: .refreshFailed(error))
        } else if let outdated {
            self.init(since: outdated.since, reason: .outdated(source: outdated.source))
        } else {
            return nil
        }
    }

    /// The time of ``since`` for the banner: the short time in the locale's style (14:02
    /// in a 24 hour locale, 2:02 PM in en_US), plus the short date when it is not today.
    ///
    /// Views pass the system values; tests pass fixed ones.
    ///
    /// - Parameters:
    ///   - now: The current time.
    ///   - calendar: The calendar that decides what today is.
    ///   - locale: The locale of the format.
    ///   - timeZone: The time zone of the format and of today.
    public func formattedSince(
        now: Date,
        calendar: Calendar,
        locale: Locale,
        timeZone: TimeZone
    ) -> String {
        var calendar = calendar
        calendar.timeZone = timeZone
        let isToday = calendar.isDate(since, inSameDayAs: now)
        let style = Date.FormatStyle(
            date: isToday ? .omitted : .numeric,
            time: .shortened,
            locale: locale,
            calendar: calendar,
            timeZone: timeZone
        )
        return since.formatted(style)
    }

    /// The banner title: "Showing the last known state from <time>." (mockup).
    func bannerTitle(now: Date, calendar: Calendar, locale: Locale, timeZone: TimeZone) -> String {
        let time = formattedSince(now: now, calendar: calendar, locale: locale, timeZone: timeZone)
        return "Showing the last known state from \(time)."
    }

    /// The banner message: "Couldn’t refresh: <message>." for a failed refresh, or
    /// "<source> last updated <time>." for outdated data.
    ///
    /// The outdated sentence always starts with a capital letter, so a `source` such as
    /// "the bundled catalog" reads "The bundled catalog last updated …"; only its first
    /// character changes.
    func bannerMessage(now: Date, calendar: Calendar, locale: Locale, timeZone: TimeZone) -> String
    {
        switch reason {
        case .refreshFailed(let failure):
            return "Couldn’t refresh: \(failure.message.withFinalPeriod)"
        case .outdated(let source):
            let time = formattedSince(
                now: now,
                calendar: calendar,
                locale: locale,
                timeZone: timeZone
            )
            return "\(source) last updated \(time)."
                .withFirstCharacterUppercased
        }
    }
}

extension String {
    /// The string with its first character in upper case, whatever the locale: a character
    /// without an upper case form (a digit, a symbol, a space) and the empty string stay
    /// as they are.
    nonisolated fileprivate var withFirstCharacterUppercased: String {
        guard let first else { return self }
        return first.uppercased() + dropFirst()
    }

    /// The string ending in one period: daemon messages have none, but a screen's copy may.
    nonisolated fileprivate var withFinalPeriod: String {
        hasSuffix(".") ? self : self + "."
    }
}
