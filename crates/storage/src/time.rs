//! The two text forms of time the registry writes.

use jiff::Timestamp;

/// Length of a stored timestamp: `YYYY-MM-DDTHH:MM:SS.fffffffffZ`.
pub(crate) const TIMESTAMP_LEN: usize = 30;

/// Formats a timestamp the way every time column stores it: UTC with
/// exactly nine fraction digits and a `Z`, 30 characters long.
pub(crate) fn format_timestamp(ts: Timestamp) -> String {
    format!("{ts:.9}")
}

/// Parses a stored timestamp, accepting only the 30 character form.
pub(crate) fn parse_timestamp(text: &str) -> Option<Timestamp> {
    if text.len() != TIMESTAMP_LEN || !text.ends_with('Z') {
        return None;
    }
    text.parse().ok()
}

/// Formats a timestamp for a file name: `YYYYMMDDTHHMMSSZ`, which sorts in
/// time order as text.
pub(crate) fn file_stamp(ts: Timestamp) -> String {
    ts.strftime("%Y%m%dT%H%M%SZ").to_string()
}

/// Whether `text` has the shape [`file_stamp`] writes.
pub(crate) fn is_file_stamp(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 16
        && bytes[8] == b'T'
        && bytes[15] == b'Z'
        && bytes[..8].iter().all(u8::is_ascii_digit)
        && bytes[9..15].iter().all(u8::is_ascii_digit)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_timestamps_have_nine_fraction_digits_and_round_trip() {
        let whole = Timestamp::constant(1_767_225_600, 0);
        let fine = Timestamp::constant(1_767_225_600, 123_456_789);

        assert_eq!(format_timestamp(whole), "2026-01-01T00:00:00.000000000Z");
        assert_eq!(format_timestamp(fine), "2026-01-01T00:00:00.123456789Z");
        for ts in [whole, fine, Timestamp::MIN, Timestamp::MAX] {
            let text = format_timestamp(ts);
            if text.len() == TIMESTAMP_LEN {
                assert_eq!(parse_timestamp(&text), Some(ts), "{text}");
            }
        }
    }

    #[test]
    fn parsing_rejects_offsets_and_other_precisions() {
        for text in [
            "2026-10-18T05:42:00+07:00",
            "2026-10-18T05:42:00.000Z",
            "2026-10-18T05:42:00.0000000000Z",
            "",
        ] {
            assert_eq!(parse_timestamp(text), None, "{text}");
        }
    }

    #[test]
    fn file_stamps_are_compact_utc() {
        let ts = Timestamp::constant(1_767_225_600, 999_999_999);

        assert_eq!(file_stamp(ts), "20260101T000000Z");
        assert!(is_file_stamp(&file_stamp(ts)));
        assert!(!is_file_stamp("20260101T000000"));
        assert!(!is_file_stamp("2026010aT000000Z"));
    }
}
