import Foundation
import StacklyClient

/// The Lease row of the Details section (spec 0047, *Copy Lease*).
extension EnvironmentCopy {
    /// The Lease row, or `nil` to hide it when the detail carries no lease (an older daemon).
    /// - Parameters:
    ///   - lease: `EnvironmentDetail.lease`.
    ///   - now: The moment the page treats as now.
    ///   - calendar: The calendar of the window.
    ///   - locale: The locale of the window.
    ///   - timeZone: The time zone of the window.
    static func leaseText(
        _ lease: EnvLeaseInfo?, now: Date, calendar: Calendar, locale: Locale,
        timeZone: TimeZone
    ) -> String? {
        guard let lease else { return nil }
        guard lease.activeCount > 0 else {
            return "No lease. Up takes one on each service’s instance."
        }
        let count = lease.activeCount == 1 ? "1 instance" : "\(lease.activeCount) instances"
        let since =
            lease.heldSince.map {
                " since "
                    + leaseTime(
                        $0, now: now, calendar: calendar, locale: locale, timeZone: timeZone)
            } ?? ""
        return
            "Lease held on \(count)\(since). Down stops only instances with no other lease or manual start."
    }

    /// When a lease was taken, as an Activity row writes it: the short time for today, else
    /// the short date and the short time.
    static func leaseTime(
        _ date: Date, now: Date, calendar: Calendar, locale: Locale, timeZone: TimeZone
    ) -> String {
        var calendar = calendar
        calendar.timeZone = timeZone
        let time = Date.FormatStyle(
            date: .omitted, time: .shortened, locale: locale, calendar: calendar,
            timeZone: timeZone)
        if calendar.isDate(date, inSameDayAs: now) {
            return date.formatted(time)
        }
        let day = ActivityCopy.time(
            date, now: now, calendar: calendar, locale: locale, timeZone: timeZone)
        return "\(day), \(date.formatted(time))"
    }
}
