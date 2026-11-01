//! The reading side of `instance.logs_follow` (spec 0027, *Follow*): a
//! cursor that reads the records after it from disk each time the writer
//! reports new ones, a scan budget at a time.
//!
//! A follower that started before the store existed adopts the store it
//! finds without a gap; one whose cursor belongs to another store gets the
//! gap `store_reset` and goes on from the start of the new one; one that
//! rotation passed gets the gap `rotated` and goes on from the oldest
//! record left.

use std::path::Path;

use stackly_platform::Filesystem;

use super::filter::LogFilter;
use super::read::{LogEntry, LogGap, LogPosition, LogReadError, ReadRequest, read_page};
use super::store::{LogCursor, LogStoreId};

/// What one step of a follower read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct FollowStep {
    /// The new entries, `seq` rising.
    pub entries: Vec<LogEntry>,
    /// Applies before the entries.
    pub gap: Option<LogGap>,
    /// More may be waiting: the step stopped at its limit or budget, so
    /// the next one should run without waiting for the writer.
    pub more: bool,
}

/// A follower's position and filter.
#[derive(Debug, Clone)]
pub struct FollowSource {
    filter: LogFilter,
    cursor: Option<LogCursor>,
}

impl FollowSource {
    /// Starts after `after`, or at the first record of a store that does
    /// not exist yet when `after` is `None`.
    #[must_use]
    pub const fn new(filter: LogFilter, after: Option<LogCursor>) -> Self {
        Self {
            filter,
            cursor: after,
        }
    }

    /// Returns where the follower is.
    #[must_use]
    pub const fn cursor(&self) -> Option<LogCursor> {
        self.cursor
    }

    /// Reads the next entries after the cursor, at most `limit` and about
    /// `byte_limit` encoded bytes, scanning at most `scan_budget`.
    ///
    /// # Errors
    ///
    /// Returns [`LogReadError`] when the store cannot be read.
    pub fn step(
        &mut self,
        fs: &dyn Filesystem,
        dir: &Path,
        limit: usize,
        byte_limit: usize,
        scan_budget: u64,
        segments_per_stream: usize,
    ) -> Result<FollowStep, LogReadError> {
        let position = self.cursor.map_or(LogPosition::Start, LogPosition::After);
        let request = ReadRequest {
            filter: self.filter.clone(),
            position,
            limit,
            byte_limit,
            scan_budget,
            segments_per_stream,
        };
        let page = read_page(fs, dir, &request)?;
        let Some(store) = page.store else {
            return Ok(FollowStep::default());
        };
        let reached = page.examined.or(page.entries.last().map(|entry| entry.seq));
        let seq = match (reached, self.cursor) {
            (Some(seq), _) => Some(seq),
            (None, Some(cursor)) if cursor.store == store && page.gap.is_none() => Some(cursor.seq),
            _ => None,
        };
        self.cursor = seq
            .map(|seq| LogCursor { store, seq })
            .or(self.cursor_in(store));
        Ok(FollowStep {
            entries: page.entries,
            gap: page.gap,
            more: page.later.is_some(),
        })
    }

    /// The cursor kept when nothing was read: the old one in the same
    /// store, else none (the store is read from its start).
    fn cursor_in(&self, store: LogStoreId) -> Option<LogCursor> {
        self.cursor.filter(|cursor| cursor.store == store)
    }
}
