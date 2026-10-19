//! The exit code map (spec 0008, *Peta exit code*).
//!
//! An exit code depends only on the error category, never on its message
//! or its code. The one exception is `cli.interrupted`, which carries
//! [`EXIT_INTERRUPTED`] as its exit override.

use stackly_protocol::ErrorCategory;

/// The exit code of a command that worked.
pub const EXIT_SUCCESS: u8 = 0;

/// The exit code of arguments clap refuses, the same as `invalid_input`.
pub const EXIT_USAGE: u8 = exit_code(ErrorCategory::InvalidInput);

/// The exit code when Ctrl+C stopped a wait; the operation keeps running.
pub const EXIT_INTERRUPTED: u8 = 130;

/// The exit code of an error `category`. The match lists every category
/// without a wildcard arm, so a new category fails to compile here.
#[must_use]
pub const fn exit_code(category: ErrorCategory) -> u8 {
    match category {
        ErrorCategory::Internal => 1,
        ErrorCategory::InvalidInput => 2,
        ErrorCategory::Conflict => 3,
        ErrorCategory::Unauthorized => 4,
        ErrorCategory::Unsupported => 5,
        ErrorCategory::Unavailable => 6,
        ErrorCategory::Failed => 7,
        ErrorCategory::Timeout => 8,
        ErrorCategory::Cancelled => 9,
        ErrorCategory::NotFound => 10,
    }
}

/// The exit code table for `stackly --help`, one line per code.
pub const EXIT_TABLE: &str = "\
Exit codes:
  0    success
  1    a bug or broken invariant in Stackly (internal)
  2    wrong input or arguments (invalid_input)
  3    conflict: the state changed or the resource is in use (conflict)
  4    identity, peer, or trust refused (unauthorized)
  5    version, method, or capability not supported (unsupported)
  6    the daemon or a dependency is not available (unavailable)
  7    the work ran and failed (failed)
  8    a time limit ran out; the result may be unknown (timeout)
  9    cancelled (cancelled)
  10   the resource does not exist (not_found)
  130  Ctrl+C stopped waiting; the operation keeps running";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_category_maps_to_the_spec_0008_exit_code() {
        let table: Vec<(&str, u8)> = ErrorCategory::ALL
            .iter()
            .map(|category| (category.as_str(), exit_code(*category)))
            .collect();
        let mut expected = vec![
            ("internal", 1),
            ("invalid_input", 2),
            ("conflict", 3),
            ("unauthorized", 4),
            ("unsupported", 5),
            ("unavailable", 6),
            ("failed", 7),
            ("timeout", 8),
            ("cancelled", 9),
            ("not_found", 10),
        ];
        let mut table = table;
        table.sort_unstable();
        expected.sort_unstable();
        assert_eq!(table, expected);
        assert_eq!(EXIT_USAGE, 2);
    }

    #[test]
    fn the_help_table_lists_every_exit_code_once() {
        let codes: Vec<&str> = EXIT_TABLE
            .lines()
            .skip(1)
            .filter_map(|line| line.split_whitespace().next())
            .collect();
        assert_eq!(
            codes,
            [
                "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "130"
            ]
        );
    }
}
