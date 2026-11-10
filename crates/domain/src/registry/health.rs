//! What the registry says about its own health, for the `registry` check
//! of the doctor (spec 0034 AC-8): the answer of `PRAGMA quick_check`, and
//! the stored values of open code sets that this build does not know.

/// The most messages a failed [`QuickCheck`] keeps.
pub const QUICK_CHECK_MESSAGES_MAX: usize = 10;

/// The most rows [`Registry::scan_unrecognized`] reports per column.
///
/// [`Registry::scan_unrecognized`]: super::Registry::scan_unrecognized
pub const UNRECOGNIZED_ROWS_PER_COLUMN: usize = 64;

/// The most bytes of a stored value an [`UnrecognizedValue`] keeps.
pub const UNRECOGNIZED_VALUE_MAX_LEN: usize = 64;

/// The answer of `PRAGMA quick_check` on the registry.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum QuickCheck {
    /// SQLite found nothing wrong.
    Passed,
    /// SQLite reported problems: its messages, at most
    /// [`QUICK_CHECK_MESSAGES_MAX`] of them, in its order.
    Failed {
        /// The messages, each as SQLite wrote it.
        messages: Vec<String>,
    },
}

impl QuickCheck {
    /// Reads the rows `PRAGMA quick_check` answered: the single row `ok`
    /// passes, and anything else fails with the first
    /// [`QUICK_CHECK_MESSAGES_MAX`] rows. No rows at all fails too, since
    /// SQLite always answers at least one.
    #[must_use]
    pub fn from_messages(mut messages: Vec<String>) -> Self {
        if messages.len() == 1 && messages[0] == "ok" {
            return Self::Passed;
        }
        if messages.is_empty() {
            messages.push("quick_check answered no rows".to_owned());
        }
        messages.truncate(QUICK_CHECK_MESSAGES_MAX);
        Self::Failed { messages }
    }

    /// True when SQLite found nothing wrong.
    #[must_use]
    pub const fn passed(&self) -> bool {
        matches!(self, Self::Passed)
    }
}

/// One stored value of an open code set that this build does not know, so
/// every read that loads its row fails as `Corrupt` (spec 0005, BR-017).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct UnrecognizedValue {
    /// The table, for example `operations`.
    pub table: &'static str,
    /// The SQLite `rowid` of the row.
    pub row_id: i64,
    /// The column, for example `kind`.
    pub column: &'static str,
    /// The stored value as text, cut to at most
    /// [`UNRECOGNIZED_VALUE_MAX_LEN`] bytes on a character boundary.
    pub value: String,
}

impl UnrecognizedValue {
    /// Records `value` found in `column` of row `row_id` of `table`, cut to
    /// [`UNRECOGNIZED_VALUE_MAX_LEN`] bytes on a character boundary.
    #[must_use]
    pub fn new(table: &'static str, row_id: i64, column: &'static str, value: &str) -> Self {
        let mut end = value.len().min(UNRECOGNIZED_VALUE_MAX_LEN);
        while !value.is_char_boundary(end) {
            end -= 1;
        }
        Self {
            table,
            row_id,
            column,
            value: value[..end].to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_single_ok_row_passes() {
        assert_eq!(
            QuickCheck::from_messages(vec!["ok".to_owned()]),
            QuickCheck::Passed
        );
        assert!(QuickCheck::from_messages(vec!["ok".to_owned()]).passed());
        assert_eq!(
            QuickCheck::from_messages(vec!["ok".to_owned(), "ok".to_owned()]),
            QuickCheck::Failed {
                messages: vec!["ok".to_owned(), "ok".to_owned()]
            }
        );
    }

    #[test]
    fn a_failure_keeps_at_most_ten_messages_in_order() {
        let messages: Vec<String> = (1..=12).map(|n| format!("row {n} missing")).collect();

        let QuickCheck::Failed { messages: kept } = QuickCheck::from_messages(messages.clone())
        else {
            panic!("twelve messages are a failure");
        };

        assert_eq!(kept, messages[..QUICK_CHECK_MESSAGES_MAX]);
    }

    #[test]
    fn no_rows_at_all_is_a_failure_that_says_so() {
        assert_eq!(
            QuickCheck::from_messages(Vec::new()),
            QuickCheck::Failed {
                messages: vec!["quick_check answered no rows".to_owned()]
            }
        );
    }

    #[test]
    fn an_unrecognized_value_is_cut_to_64_bytes_on_a_character_boundary() {
        let short = UnrecognizedValue::new("operations", 7, "kind", "teleport");
        assert_eq!(short.value, "teleport");
        assert_eq!(
            (short.table, short.row_id, short.column),
            ("operations", 7, "kind")
        );

        let long = "a".repeat(63) + "é" + "tail";
        let cut = UnrecognizedValue::new("operations", 7, "kind", &long);
        assert_eq!(cut.value, "a".repeat(63), "the two byte é would pass 64");

        let exact = "b".repeat(80);
        assert_eq!(
            UnrecognizedValue::new("operations", 7, "kind", &exact).value,
            "b".repeat(UNRECOGNIZED_VALUE_MAX_LEN)
        );
    }
}
