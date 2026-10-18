import Foundation
import Testing

@testable import StacklyUI

/// Times in the shell follow the system locale and time zone through pure functions
/// (spec 0013 AC-22): en_US and a 24 hour locale, two time zones, today and yesterday.
struct ShellTimeTests {
    /// 2026-10-17 14:02:00 UTC.
    nonisolated static let now = Date(timeIntervalSince1970: 1_792_245_720)
    nonisolated static let gregorian = Calendar(identifier: .gregorian)
    nonisolated static let us = Locale(identifier: "en_US")
    nonisolated static let british = Locale(identifier: "en_GB")
    nonisolated static let jakarta = TimeZone(identifier: "Asia/Jakarta") ?? .gmt

    static func bannerMessage(lastKnownAt: Date, locale: Locale, timeZone: TimeZone) -> String? {
        ShellPresentation(status: .unreachable(lastKnownAt: lastKnownAt, since: now))
            .banner?
            .message(now: now, calendar: gregorian, locale: locale, timeZone: timeZone)
    }

    @Test func bannerShowsTodaysTimeInTheLocalesStyleAndZone() {
        let tail = " Actions are disabled until the daemon is reachable."
        #expect(
            Self.bannerMessage(lastKnownAt: Self.now, locale: Self.us, timeZone: .gmt)
                == "Showing the last known state from 2:02\u{202F}PM." + tail
        )
        #expect(
            Self.bannerMessage(lastKnownAt: Self.now, locale: Self.british, timeZone: .gmt)
                == "Showing the last known state from 14:02." + tail
        )
        #expect(
            Self.bannerMessage(lastKnownAt: Self.now, locale: Self.british, timeZone: Self.jakarta)
                == "Showing the last known state from 21:02." + tail
        )
    }

    @Test func bannerAddsTheDateForYesterday() {
        let yesterday = Self.now.addingTimeInterval(-86_400)
        let message = Self.bannerMessage(
            lastKnownAt: yesterday,
            locale: Self.us,
            timeZone: .gmt
        )
        #expect(
            message?.hasPrefix("Showing the last known state from 10/16/2026, 2:02\u{202F}PM.")
                == true)
    }

    @Test func diagnosticsTimeShowsTheDateAndSecondsInEachLocaleAndZone() {
        let date = Self.now.addingTimeInterval(7)
        #expect(
            DiagnosticsTime.format(date, calendar: Self.gregorian, locale: Self.us, timeZone: .gmt)
                == "Oct 17, 2026 at 2:02:07\u{202F}PM"
        )
        #expect(
            DiagnosticsTime.format(
                date,
                calendar: Self.gregorian,
                locale: Self.british,
                timeZone: .gmt
            ) == "17 Oct 2026 at 14:02:07"
        )
        #expect(
            DiagnosticsTime.format(
                date,
                calendar: Self.gregorian,
                locale: Self.british,
                timeZone: Self.jakarta
            ) == "17 Oct 2026 at 21:02:07"
        )
    }

    @Test func diagnosticsTimeKeepsTheDateAcrossMidnightInAnotherZone() {
        // 22:30 UTC on Oct 17 is already Oct 18 in Jakarta.
        let late = Date(timeIntervalSince1970: 1_792_276_200)
        #expect(
            DiagnosticsTime.format(late, calendar: Self.gregorian, locale: Self.us, timeZone: .gmt)
                == "Oct 17, 2026 at 10:30:00\u{202F}PM"
        )
        #expect(
            DiagnosticsTime.format(
                late,
                calendar: Self.gregorian,
                locale: Self.british,
                timeZone: Self.jakarta
            ) == "18 Oct 2026 at 5:30:00"
        )
    }
}
