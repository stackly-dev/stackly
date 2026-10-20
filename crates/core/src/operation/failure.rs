//! Shrinking the items of a stored failure to what the `failure_items`
//! column holds (spec 0007, "Kegagalan dan error"): at most
//! [`FailureError::ITEMS_MAX`] items, at most
//! [`FAILURE_ITEMS_MAX_BYTES`] bytes in their stored form.

use serde_json::{Map, Value};
use stackly_domain::{DetailKey, FailureError, FailureItem};

#[cfg(test)]
use super::limits::FAILURE_ITEMS_MAX_BYTES;
use super::redact::Redact;
use super::state::pair;

/// The items that fit, and how many were left out.
pub(super) struct Fitted {
    pub(super) items: Vec<FailureItem>,
    pub(super) omitted: u64,
}

/// Redacts and shortens the field values of `items`, drops fields above
/// [`FailureItem::FIELDS_MAX`], then keeps items from the front while they
/// number at most [`FailureError::ITEMS_MAX`] and their stored form stays
/// within `max_bytes`.
pub(super) fn fit(items: Vec<FailureItem>, redactor: &dyn Redact, max_bytes: usize) -> Fitted {
    let total = items.len() as u64;
    let mut kept = Vec::new();
    // The stored form is `[` item `,` item ... `]`.
    let mut size = 2_usize;
    for item in items {
        if kept.len() >= FailureError::ITEMS_MAX {
            break;
        }
        let Some(item) = clean(item, redactor) else {
            continue;
        };
        let item_size = stored_len(&item) + usize::from(!kept.is_empty());
        if size + item_size > max_bytes {
            break;
        }
        size += item_size;
        kept.push(item);
    }
    Fitted {
        omitted: total - kept.len() as u64,
        items: kept,
    }
}

/// Adds `omitted` to the `omitted` detail the work may already carry.
pub(super) fn add_omitted(details: &mut Vec<(DetailKey, String)>, omitted: u64) {
    if omitted == 0 {
        return;
    }
    match details
        .iter_mut()
        .find(|(key, _)| key.as_str() == "omitted")
    {
        Some((_, value)) => {
            let before = value.parse::<u64>().unwrap_or(0);
            *value = before.saturating_add(omitted).to_string();
        }
        None => details.push(pair("omitted", omitted)),
    }
}

fn clean(item: FailureItem, redactor: &dyn Redact) -> Option<FailureItem> {
    let fields = item
        .fields()
        .iter()
        .take(FailureItem::FIELDS_MAX)
        .map(|(key, value)| {
            let value = redactor.redact(value);
            (
                key.clone(),
                super::state::truncate_bytes(&value, FailureError::DETAIL_VALUE_MAX),
            )
        })
        .collect();
    FailureItem::new(item.kind().clone(), fields).ok()
}

/// The size of one item in the `failure_items` column: compact JSON
/// `{"fields":[{"key":..,"value":..}],"kind":..}`, as `serde_json` writes
/// it.
fn stored_len(item: &FailureItem) -> usize {
    let fields: Vec<Value> = item
        .fields()
        .iter()
        .map(|(key, value)| {
            let mut row = Map::new();
            row.insert("key".to_owned(), Value::String(key.to_string()));
            row.insert("value".to_owned(), Value::String(value.clone()));
            Value::Object(row)
        })
        .collect();
    let mut row = Map::new();
    row.insert("kind".to_owned(), Value::String(item.kind().to_string()));
    row.insert("fields".to_owned(), Value::Array(fields));
    serde_json::to_string(&Value::Object(row)).map_or(usize::MAX, |text| text.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Plain;

    impl Redact for Plain {
        fn redact(&self, text: &str) -> String {
            text.to_owned()
        }
    }

    fn item(value: &str) -> FailureItem {
        FailureItem::new(
            "issue".parse().unwrap(),
            vec![("path".parse().unwrap(), value.to_owned())],
        )
        .unwrap()
    }

    #[test]
    fn items_above_one_hundred_are_dropped_from_the_back_and_counted() {
        let items = (0..101).map(|n| item(&n.to_string())).collect();

        let fitted = fit(items, &Plain, FAILURE_ITEMS_MAX_BYTES);

        assert_eq!(fitted.items.len(), 100);
        assert_eq!(fitted.omitted, 1);
        assert_eq!(fitted.items[99].fields()[0].1, "99");
    }

    #[test]
    fn items_past_the_byte_limit_are_dropped_from_the_back_and_counted() {
        let big = "v".repeat(512);
        let items: Vec<FailureItem> = (0..100).map(|_| item(&big)).collect();
        let one = stored_len(&items[0]);

        let fitted = fit(items, &Plain, 2 + one * 3 + 2);

        assert_eq!(fitted.items.len(), 3);
        assert_eq!(fitted.omitted, 97);
        let stored: usize = 2 + fitted.items.iter().map(stored_len).sum::<usize>() + 2;
        assert!(stored <= 2 + one * 3 + 2);
    }

    #[test]
    fn the_size_matches_what_the_registry_writes() {
        let items = [item("a"), item("é\"b")];
        let rows: Vec<Value> = items
            .iter()
            .map(|item| {
                serde_json::json!({
                    "kind": item.kind().as_str(),
                    "fields": item.fields().iter()
                        .map(|(k, v)| serde_json::json!({"key": k.as_str(), "value": v}))
                        .collect::<Vec<_>>(),
                })
            })
            .collect();
        let text = serde_json::to_string(&rows).unwrap();

        let sum: usize = items.iter().map(stored_len).sum();

        assert_eq!(text.len(), 2 + sum + 1);
    }

    #[test]
    fn omitted_adds_to_the_count_the_work_already_carried() {
        let mut details = vec![pair("omitted", 4)];

        add_omitted(&mut details, 3);
        assert_eq!(details[0].1, "7");

        let mut none = Vec::new();
        add_omitted(&mut none, 0);
        assert!(none.is_empty());
        add_omitted(&mut none, 2);
        assert_eq!(none, vec![pair("omitted", 2)]);
    }
}
