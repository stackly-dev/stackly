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

/// Draws from `source` until the short key of the ID is none of `known`,
/// so the folder of the run (`i/<short key>`) is never shared.
///
/// # Panics
///
/// Panics when `source` keeps returning IDs whose short key is taken,
/// after 1024 draws; with real IDs the chance is nil.
#[must_use]
pub fn fresh_instance_id(source: &dyn IdSource, known: &[InstanceId]) -> InstanceId {
    for _ in 0..1024 {
        let id = source.next();
        if known
            .iter()
            .all(|other| other.short_key() != id.short_key())
        {
            return id;
        }
    }
    panic!("the ID source kept returning IDs whose short key is taken");
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

        assert_eq!(drawn, free);
    }

    #[test]
    fn a_scripted_source_hands_out_its_ids_first_and_then_real_ones() {
        let first = InstanceId::generate();
        let source = ScriptedIds::new([first]);

        assert_eq!(source.next(), first);
        assert_ne!(source.next(), first);
        assert_eq!(fresh_instance_id(&SystemIds, &[]).to_string().len(), 36);
    }
}
