//! Time for people (spec 0008, *Output manusia*): absolute times in the
//! system time zone, ages, and durations. Pure functions, so tests pass a
//! fixed `now` and zone.

use std::time::Duration;

use jiff::Timestamp;
use jiff::tz::TimeZone;

/// `ts` in `tz` as `YYYY-MM-DD HH:MM:SS ±HH:MM`, such as
/// `2026-10-18 10:04:05 +07:00`.
#[must_use]
pub fn format_local(ts: Timestamp, tz: &TimeZone) -> String {
    ts.to_zoned(tz.clone())
        .strftime("%Y-%m-%d %H:%M:%S %:z")
        .to_string()
}

/// How long ago `ts` was at `now`, in its largest whole unit: `now` under
/// one second or in the future, then `<n>s ago`, `<n>m ago`, `<n>h ago`,
/// or `<n>d ago`, rounded down.
#[must_use]
pub fn format_age(ts: Timestamp, now: Timestamp) -> String {
    let seconds = now.duration_since(ts).as_secs();
    if seconds < 1 {
        "now".to_owned()
    } else if seconds < 60 {
        format!("{seconds}s ago")
    } else if seconds < 3_600 {
        format!("{}m ago", seconds / 60)
    } else if seconds < 86_400 {
        format!("{}h ago", seconds / 3_600)
    } else {
        format!("{}d ago", seconds / 86_400)
    }
}

/// `duration` in its two largest units without a space, the second one
/// padded: `12s`, `3m04s`, `1h02m`, `2d05h`. Under a second is `0s`.
#[must_use]
pub fn format_duration(duration: Duration) -> String {
    let seconds = duration.as_secs();
    let (days, hours, minutes, secs) = (
        seconds / 86_400,
        (seconds / 3_600) % 24,
        (seconds / 60) % 60,
        seconds % 60,
    );
    if days > 0 {
        format!("{days}d{hours:02}h")
    } else if hours > 0 {
        format!("{hours}h{minutes:02}m")
    } else if minutes > 0 {
        format!("{minutes}m{secs:02}s")
    } else {
        format!("{secs}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(text: &str) -> Timestamp {
        text.parse().unwrap()
    }

    #[test]
    fn local_time_follows_the_zone_and_names_its_offset() {
        let at = ts("2026-10-18T03:04:05.678Z");
        let jakarta = TimeZone::get("Asia/Jakarta").unwrap();
        assert_eq!(format_local(at, &jakarta), "2026-10-18 10:04:05 +07:00");
        assert_eq!(
            format_local(at, &TimeZone::UTC),
            "2026-10-18 03:04:05 +00:00"
        );
        let kolkata = TimeZone::get("Asia/Kolkata").unwrap();
        assert_eq!(format_local(at, &kolkata), "2026-10-18 08:34:05 +05:30");
    }

    #[test]
    fn ages_use_the_largest_whole_unit_and_now_for_the_future() {
        let now = ts("2026-10-18T03:04:05Z");
        let cases = [
            ("2026-10-18T03:04:05Z", "now"),
            ("2026-10-18T03:04:04.500Z", "now"),
            ("2026-10-18T03:05:00Z", "now"),
            ("2026-10-18T03:04:04Z", "1s ago"),
            ("2026-10-18T03:03:06Z", "59s ago"),
            ("2026-10-18T03:03:05Z", "1m ago"),
            ("2026-10-18T02:04:06Z", "59m ago"),
            ("2026-10-18T02:04:05Z", "1h ago"),
            ("2026-10-17T03:04:06Z", "23h ago"),
            ("2026-10-17T03:04:05Z", "1d ago"),
            ("2026-09-18T03:04:05Z", "30d ago"),
        ];
        for (at, expected) in cases {
            assert_eq!(format_age(ts(at), now), expected, "{at}");
        }
    }

    #[test]
    fn durations_show_their_two_largest_units() {
        let cases = [
            (0, "0s"),
            (12, "12s"),
            (59, "59s"),
            (60, "1m00s"),
            (184, "3m04s"),
            (3_720, "1h02m"),
            (86_399, "23h59m"),
            (190_800, "2d05h"),
        ];
        for (seconds, expected) in cases {
            assert_eq!(format_duration(Duration::from_secs(seconds)), expected);
        }
        assert_eq!(format_duration(Duration::from_millis(999)), "0s");
    }
}
