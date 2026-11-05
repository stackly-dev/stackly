//! Where the ID of a new instance comes from (spec 0020, *Operasi
//! `create_instance`*): the handler draws until the short key is free, and
//! a test can script what it draws.

use stackly_domain::InstanceId;

/// A source of new instance IDs.
pub trait IdSource: Send + Sync {
    /// Returns a new ID. Only the daemon creates IDs, so only a test
    /// source returns anything but `InstanceId::generate()`.
    fn next(&self) -> InstanceId;
}

/// The IDs the daemon makes: UUID version 7 from the system time.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemIds;

impl IdSource for SystemIds {
    fn next(&self) -> InstanceId {
        InstanceId::generate()
    }
}

/// How many IDs [`fresh_instance_id`] draws before it gives up.
const MAX_DRAWS: usize = 1024;

/// Draws from `source` until the short key of the ID is none of `known`,
/// so the folder of the run (`i/<short key>`) is never shared. Returns
/// `None` when `source` keeps returning IDs whose short key is taken, after
/// 1024 draws; with real IDs the chance is nil.
#[must_use]
pub fn fresh_instance_id(source: &dyn IdSource, known: &[InstanceId]) -> Option<InstanceId> {
    (0..MAX_DRAWS).map(|_| source.next()).find(|id| {
        known
            .iter()
            .all(|other| other.short_key() != id.short_key())
    })
}

/// An ID source that returns scripted IDs first, then real ones, for
/// tests that force an ID (an orphan folder, a taken short key).
#[cfg(feature = "test-support")]
#[derive(Debug, Default)]
pub struct ScriptedIds {
    queue: std::sync::Mutex<std::collections::VecDeque<InstanceId>>,
}

#[cfg(feature = "test-support")]
impl ScriptedIds {
    /// A source that returns `ids` in order, then real IDs.
    #[must_use]
    pub fn new(ids: impl IntoIterator<Item = InstanceId>) -> Self {
        Self {
            queue: std::sync::Mutex::new(ids.into_iter().collect()),
        }
    }
}

#[cfg(feature = "test-support")]
impl IdSource for ScriptedIds {
    fn next(&self) -> InstanceId {
        self.queue
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .pop_front()
            .unwrap_or_else(InstanceId::generate)
    }
}

#[cfg(all(test, feature = "test-support"))]
mod tests {
    use super::*;

    /// Another valid ID with the same short key as `id`.
    fn same_short_key(id: InstanceId) -> InstanceId {
        let fresh = InstanceId::generate().to_string();
        let text = id.to_string();
        format!("{}{}", &fresh[..28], &text[28..])
            .parse()
            .expect("the constructed ID is valid")
    }

    #[test]
    fn an_id_whose_short_key_is_taken_is_drawn_again() {
        let known = InstanceId::generate();
        let clash = same_short_key(known);
        let free = InstanceId::generate();
        let source = ScriptedIds::new([clash, free]);

        let drawn = fresh_instance_id(&source, &[known]);

        assert_eq!(drawn, Some(free));
    }

    #[test]
    fn a_source_that_only_returns_taken_short_keys_gives_no_id() {
        struct Clashing(InstanceId);
        impl IdSource for Clashing {
            fn next(&self) -> InstanceId {
                same_short_key(self.0)
            }
        }
        let known = InstanceId::generate();

        assert_eq!(fresh_instance_id(&Clashing(known), &[known]), None);
    }

    #[test]
    fn a_scripted_source_hands_out_its_ids_first_and_then_real_ones() {
        let first = InstanceId::generate();
        let source = ScriptedIds::new([first]);

        assert_eq!(source.next(), first);
        assert_ne!(source.next(), first);
        let drawn = fresh_instance_id(&SystemIds, &[]).unwrap();
        assert_eq!(drawn.to_string().len(), 36);
    }
}
