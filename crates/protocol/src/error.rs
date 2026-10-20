//! The one error envelope on the wire (VAL-020): [`ErrorBody`] with a
//! stable [`ErrorCategory`] and a code from the [`codes`](crate::codes)
//! catalog.
//!
//! An [`ErrorBody`] is only built through [`ErrorBody::new`], so its
//! category and `retryable` always come from the catalog, never from the
//! place the error happened.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use typeshare::typeshare;
use uuid::Uuid;

use crate::codes::{self, ErrorSpec, ItemSpec};
use crate::json::FieldCode;
use crate::limits::{FIELD_ERRORS_MAX, FIELD_PATH_MAX};
use crate::wire::ResourceRef;

/// Longest `message`, in characters; [`ErrorBody::new`] truncates longer
/// ones on a character boundary.
pub const MESSAGE_MAX: usize = 200;

/// The `details` key that counts the list entries an error dropped: the
/// items [`ErrorBody::with_items`] dropped above the catalog limit, or the
/// fields [`ErrorBody::with_fields`] dropped above [`FIELD_ERRORS_MAX`].
/// Every code with `items` lists it, and so does every code with `fields`.
pub const OMITTED_DETAIL: &str = "omitted";

/// What a client should do about an error. Closed: a new category is a new
/// protocol major. Feature 8 derives CLI exit codes from it.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCategory {
    /// Fix the input; sending the same request again fails again.
    InvalidInput,
    /// The resource does not exist; refresh the snapshot.
    NotFound,
    /// State changed or the resource is in use; refresh, then decide.
    Conflict,
    /// Identity, peer, or trust was refused; never retry on its own.
    Unauthorized,
    /// The version, method, or capability is not supported.
    Unsupported,
    /// The daemon or a dependency is not available yet; check `retryable`.
    Unavailable,
    /// The work ran and failed.
    Failed,
    /// A time limit passed; the outcome may be unknown, check the operation.
    Timeout,
    /// The work was cancelled.
    Cancelled,
    /// A bug or a broken invariant in Stackly.
    Internal,
}

impl ErrorCategory {
    /// Every category, in the order of spec 0006.
    pub const ALL: [Self; 10] = [
        Self::InvalidInput,
        Self::NotFound,
        Self::Conflict,
        Self::Unauthorized,
        Self::Unsupported,
        Self::Unavailable,
        Self::Failed,
        Self::Timeout,
        Self::Cancelled,
        Self::Internal,
    ];

    /// The wire value, such as `invalid_input`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidInput => "invalid_input",
            Self::NotFound => "not_found",
            Self::Conflict => "conflict",
            Self::Unauthorized => "unauthorized",
            Self::Unsupported => "unsupported",
            Self::Unavailable => "unavailable",
            Self::Failed => "failed",
            Self::Timeout => "timeout",
            Self::Cancelled => "cancelled",
            Self::Internal => "internal",
        }
    }
}

/// The error envelope every daemon error and every client side IPC error
/// uses (VAL-020).
///
/// `code`, `category`, `message`, `retryable`, and `details` are frozen
/// across majors, so peers of any major can always exchange
/// `protocol.major_mismatch`.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ErrorBody {
    /// A catalog code, `<area>.<reason>`.
    code: String,
    /// The category the catalog gives the code.
    category: ErrorCategory,
    /// One safe English sentence: lowercase start, no trailing period, at
    /// most 200 characters, never a secret.
    message: String,
    /// True only when the daemon guarantees the request had no effect, so
    /// the exact same request may be sent again.
    retryable: bool,
    /// The resource concerned.
    #[serde(default)]
    resource: Option<ResourceRef>,
    /// The params fields that failed, for `request.invalid_params`.
    #[serde(default)]
    fields: Vec<FieldError>,
    /// A safe recovery step, in English.
    #[serde(default)]
    hint: Option<String>,
    /// Extra facts, with keys only from the code's catalog entry.
    #[serde(default)]
    details: Vec<ErrorDetail>,
    /// The operation that failed (spec 0007).
    #[serde(default)]
    operation_id: Option<Uuid>,
    /// The PRD rule ID behind the error, such as `BR-008`.
    #[serde(default)]
    rule: Option<String>,
    /// A structured list that does not fit in `details`, with the item
    /// `kind` and field keys from the code's catalog entry.
    #[serde(default)]
    items: Vec<ErrorItem>,
}

impl ErrorBody {
    /// An error for `spec` with `message`, truncated to [`MESSAGE_MAX`]
    /// characters. Category and `retryable` come from the catalog entry.
    #[must_use]
    pub fn new(spec: &ErrorSpec, message: impl Into<String>) -> Self {
        Self {
            code: spec.code.to_owned(),
            category: spec.category,
            message: truncate(message.into(), MESSAGE_MAX),
            retryable: spec.retryable,
            resource: None,
            fields: Vec::new(),
            hint: None,
            details: Vec::new(),
            operation_id: None,
            rule: None,
            items: Vec::new(),
        }
    }

    /// Adds the resource concerned.
    #[must_use]
    pub fn with_resource(mut self, resource: ResourceRef) -> Self {
        self.resource = Some(resource);
        self
    }

    /// Adds one failed params field, like [`with_fields`](Self::with_fields).
    #[must_use]
    pub fn with_field(self, field: FieldError) -> Self {
        self.with_fields([field])
    }

    /// Adds failed params fields. Fields above [`FIELD_ERRORS_MAX`] are
    /// dropped, and how many is written to the [`OMITTED_DETAIL`] detail,
    /// so an error frame stays small whatever the input. A code that
    /// carries fields must list that detail in its catalog entry and allow
    /// no items, because both lists would share the one count; debug builds
    /// assert both whenever a field is added.
    #[must_use]
    pub fn with_fields(mut self, fields: impl IntoIterator<Item = FieldError>) -> Self {
        let mut omitted: u64 = 0;
        for field in fields {
            debug_assert!(
                codes::lookup(&self.code).is_some_and(|spec| {
                    spec.items.is_none() && spec.details.contains(&OMITTED_DETAIL)
                }),
                "the catalog entry of `{}` must list `omitted` and allow no items to carry fields",
                self.code
            );
            if self.fields.len() < FIELD_ERRORS_MAX {
                self.fields.push(field);
            } else {
                omitted += 1;
            }
        }
        if omitted > 0 {
            self.add_omitted(omitted);
        }
        self
    }

    /// Adds a safe recovery step.
    #[must_use]
    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    /// Adds one detail. `key` must be one of the code's catalog `details`
    /// keys; debug builds assert it.
    #[must_use]
    pub fn with_detail(mut self, key: &str, value: impl Into<String>) -> Self {
        debug_assert!(
            codes::lookup(&self.code).is_some_and(|spec| spec.details.contains(&key)),
            "detail key `{key}` is not in the catalog entry of `{}`",
            self.code
        );
        self.details.push(ErrorDetail {
            key: key.to_owned(),
            value: value.into(),
        });
        self
    }

    /// Adds the operation that failed.
    #[must_use]
    pub fn with_operation_id(mut self, operation_id: Uuid) -> Self {
        self.operation_id = Some(operation_id);
        self
    }

    /// Adds the PRD rule ID behind the error.
    #[must_use]
    pub fn with_rule(mut self, rule: impl Into<String>) -> Self {
        self.rule = Some(rule.into());
        self
    }

    /// Adds list items. Each item's `kind` and field keys must be the ones
    /// the code's catalog entry allows, and the code must allow items at
    /// all; debug builds assert both, release builds drop such items.
    /// Items above the catalog limit are dropped, and how many is written
    /// to the [`OMITTED_DETAIL`] detail.
    #[must_use]
    pub fn with_items(self, items: impl IntoIterator<Item = ErrorItem>) -> Self {
        let spec = codes::lookup(&self.code).and_then(|spec| spec.items.as_ref());
        debug_assert!(
            spec.is_some(),
            "the catalog entry of `{}` allows no items",
            self.code
        );
        match spec {
            Some(spec) => self.push_items(spec, items),
            None => self,
        }
    }

    /// [`with_items`](Self::with_items) against `spec`.
    fn push_items(mut self, spec: &ItemSpec, items: impl IntoIterator<Item = ErrorItem>) -> Self {
        let mut omitted: u64 = 0;
        for item in items {
            let allowed = item.kind == spec.kind
                && item
                    .fields
                    .iter()
                    .all(|field| spec.fields.contains(&field.key.as_str()));
            debug_assert!(
                allowed,
                "item `{}` is not allowed by the catalog entry of `{}`",
                item.kind, self.code
            );
            if !allowed {
                continue;
            }
            if self.items.len() < usize::from(spec.max) {
                self.items.push(item);
            } else {
                omitted += 1;
            }
        }
        if omitted > 0 {
            self.add_omitted(omitted);
        }
        self
    }

    /// Returns this error with `redact` applied to its message, hint,
    /// detail values, and item field values: every free text it carries.
    /// The code, category, field errors (fixed text), and detail keys are
    /// not text a secret can reach. The message is cut to [`MESSAGE_MAX`]
    /// again, since a mask can be longer than the text it hides. The
    /// daemon calls this with its `Redactor` on every error it sends
    /// (spec 0011 AC-14).
    #[must_use]
    pub fn redacted_with(mut self, redact: impl Fn(&str) -> String) -> Self {
        self.message = truncate(redact(&self.message), MESSAGE_MAX);
        self.hint = self.hint.map(|hint| redact(&hint));
        for detail in &mut self.details {
            detail.value = redact(&detail.value);
        }
        for item in &mut self.items {
            for field in &mut item.fields {
                field.value = redact(&field.value);
            }
        }
        self
    }

    /// Adds `omitted` to the [`OMITTED_DETAIL`] detail, creating it at the
    /// first drop. Debug builds assert that the catalog entry lists it.
    fn add_omitted(&mut self, omitted: u64) {
        match self
            .details
            .iter_mut()
            .find(|detail| detail.key == OMITTED_DETAIL)
        {
            Some(detail) => {
                let before: u64 = detail.value.parse().unwrap_or(0);
                detail.value = (before + omitted).to_string();
            }
            None => {
                debug_assert!(
                    codes::lookup(&self.code)
                        .is_some_and(|spec| spec.details.contains(&OMITTED_DETAIL)),
                    "the catalog entry of `{}` does not list the `omitted` detail",
                    self.code
                );
                self.details
                    .push(ErrorDetail::new(OMITTED_DETAIL, omitted.to_string()));
            }
        }
    }

    /// The catalog code, `<area>.<reason>`.
    #[must_use]
    pub fn code(&self) -> &str {
        &self.code
    }

    /// Whether this error carries the code of `spec`.
    #[must_use]
    pub fn is(&self, spec: &ErrorSpec) -> bool {
        self.code == spec.code
    }

    /// The catalog entry of the code, or `None` for a code this build does
    /// not know (a newer peer).
    #[must_use]
    pub fn spec(&self) -> Option<&'static ErrorSpec> {
        codes::lookup(&self.code)
    }

    /// The category.
    #[must_use]
    pub const fn category(&self) -> ErrorCategory {
        self.category
    }

    /// The message.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Whether the exact same request may be sent again.
    #[must_use]
    pub const fn retryable(&self) -> bool {
        self.retryable
    }

    /// The resource concerned, if any.
    #[must_use]
    pub const fn resource(&self) -> Option<&ResourceRef> {
        self.resource.as_ref()
    }

    /// The failed params fields.
    #[must_use]
    pub fn fields(&self) -> &[FieldError] {
        &self.fields
    }

    /// The recovery step, if any.
    #[must_use]
    pub fn hint(&self) -> Option<&str> {
        self.hint.as_deref()
    }

    /// Every detail, in the order they were added.
    #[must_use]
    pub fn details(&self) -> &[ErrorDetail] {
        &self.details
    }

    /// The value of the first detail named `key`.
    #[must_use]
    pub fn detail(&self, key: &str) -> Option<&str> {
        self.details
            .iter()
            .find(|detail| detail.key == key)
            .map(|detail| detail.value.as_str())
    }

    /// The operation that failed, if any.
    #[must_use]
    pub const fn operation_id(&self) -> Option<Uuid> {
        self.operation_id
    }

    /// The PRD rule ID, if any.
    #[must_use]
    pub fn rule(&self) -> Option<&str> {
        self.rule.as_deref()
    }

    /// The list items, in the order they were added.
    #[must_use]
    pub fn items(&self) -> &[ErrorItem] {
        &self.items
    }
}

/// One params field that failed to decode or validate. An error carries at
/// most 32 of them (`FIELD_ERRORS_MAX`) and counts the rest in its
/// `omitted` detail.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FieldError {
    /// Where the field sits, relative to the params, such as `name`,
    /// `since.sequence`, or `kinds[3]`. At most 256 characters.
    path: String,
    /// A code from the fixed set in [`FieldCode`].
    code: String,
    /// The fixed text of the code; never the input value.
    message: String,
}

impl FieldError {
    /// A failed field at `path` with the fixed code and text of `code`.
    /// `path` is cut to [`FIELD_PATH_MAX`] characters, so a long map key
    /// in the input never makes the error large.
    #[must_use]
    pub fn new(path: impl Into<String>, code: FieldCode) -> Self {
        Self {
            path: truncate(path.into(), FIELD_PATH_MAX),
            code: code.as_str().to_owned(),
            message: code.message().to_owned(),
        }
    }

    /// The path, relative to the params.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// The field code, such as `too_long`.
    #[must_use]
    pub fn code(&self) -> &str {
        &self.code
    }

    /// The fixed text of the code.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

/// One extra fact on an error, as a key and a string value.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ErrorDetail {
    /// A camelCase key from the code's catalog entry.
    key: String,
    /// The value, as text.
    value: String,
}

impl ErrorDetail {
    /// A detail. [`ErrorBody::with_detail`] checks the key against the
    /// catalog; [`ErrorItem::with_field`] uses this for item fields.
    #[must_use]
    pub fn new(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
        }
    }

    /// The key.
    #[must_use]
    pub fn key(&self) -> &str {
        &self.key
    }

    /// The value.
    #[must_use]
    pub fn value(&self) -> &str {
        &self.value
    }
}

/// One entry of a structured list on an error, such as one reference that
/// needs a secret (spec 0011) or one manifest issue (spec 0009).
///
/// `kind` and the field keys are plain strings, so a client always decodes
/// an error frame, even with an item kind it does not know yet.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ErrorItem {
    /// The item kind from the code's catalog entry, such as `issue`.
    kind: String,
    /// The item's facts, with keys from the code's catalog entry.
    #[serde(default)]
    fields: Vec<ErrorDetail>,
}

impl ErrorItem {
    /// An item of `kind` without fields yet.
    #[must_use]
    pub fn new(kind: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            fields: Vec::new(),
        }
    }

    /// Adds one field. [`ErrorBody::with_items`] checks the key against the
    /// catalog.
    #[must_use]
    pub fn with_field(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.fields.push(ErrorDetail::new(key, value));
        self
    }

    /// The item kind.
    #[must_use]
    pub fn kind(&self) -> &str {
        &self.kind
    }

    /// The item's fields, in the order they were added.
    #[must_use]
    pub fn fields(&self) -> &[ErrorDetail] {
        &self.fields
    }

    /// The value of the first field named `key`.
    #[must_use]
    pub fn field(&self, key: &str) -> Option<&str> {
        self.fields
            .iter()
            .find(|field| field.key == key)
            .map(|field| field.value.as_str())
    }
}

/// `text` cut to at most `max` characters, on a character boundary.
fn truncate(mut text: String, max: usize) -> String {
    if let Some((end, _)) = text.char_indices().nth(max) {
        text.truncate(end);
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codes::{request, resource};
    use crate::wire::ResourceKind;

    #[test]
    fn category_and_retryable_come_from_the_catalog_entry() {
        let error = ErrorBody::new(&request::TOO_MANY_IN_FLIGHT, "too many requests in flight");
        assert_eq!(error.code(), "request.too_many_in_flight");
        assert_eq!(error.category(), ErrorCategory::Unavailable);
        assert!(error.retryable());
        assert!(error.is(&request::TOO_MANY_IN_FLIGHT));
        assert_eq!(error.spec(), Some(&request::TOO_MANY_IN_FLIGHT));
    }

    #[test]
    fn redacting_rewrites_every_free_text_and_keeps_the_rest() {
        let error = ErrorBody::new(&request::TOO_MANY_IN_FLIGHT, "token hunter2 leaked")
            .with_hint("hunter2 is not a hint")
            .with_detail("max", "hunter2");
        let redacted = error
            .clone()
            .redacted_with(|text| text.replace("hunter2", "****"));
        assert_eq!(redacted.message(), "token **** leaked");
        assert_eq!(redacted.hint(), Some("**** is not a hint"));
        assert_eq!(redacted.detail("max"), Some("****"));
        assert_eq!(redacted.code(), error.code());
        assert_eq!(redacted.category(), error.category());
        assert_eq!(redacted.retryable(), error.retryable());
    }

    #[test]
    fn redacting_cuts_a_message_that_a_mask_made_longer() {
        let error = ErrorBody::new(&request::MALFORMED, "x".repeat(200));
        let redacted = error.redacted_with(|text| text.replace('x', "xx"));
        assert_eq!(redacted.message().chars().count(), 200);
    }

    #[test]
    fn a_message_of_300_characters_is_cut_to_200() {
        let error = ErrorBody::new(&request::MALFORMED, "é".repeat(300));
        assert_eq!(error.message().chars().count(), MESSAGE_MAX);
        assert_eq!(error.message(), "é".repeat(200));
    }

    #[test]
    fn a_short_message_is_kept_as_is() {
        let error = ErrorBody::new(&request::MALFORMED, "request is not valid json");
        assert_eq!(error.message(), "request is not valid json");
    }

    #[test]
    fn builders_fill_every_optional_field() {
        let operation = Uuid::nil();
        let error = ErrorBody::new(&resource::REVISION_MISMATCH, "the revision changed")
            .with_resource(ResourceRef {
                kind: ResourceKind::Instance,
                id: "0192f3a0-7c1e-7b2a-9d4e-5f6a7b8c9d0e".to_owned(),
            })
            .with_detail("expected", "3")
            .with_detail("actual", "4")
            .with_hint("refresh the instance, then try again")
            .with_operation_id(operation)
            .with_rule("BR-008");
        let invalid = ErrorBody::new(&request::INVALID_PARAMS, "params do not match the method")
            .with_field(FieldError::new("name", FieldCode::TooLong));
        assert_eq!(error.detail("expected"), Some("3"));
        assert_eq!(error.detail("actual"), Some("4"));
        assert_eq!(error.detail("missing"), None);
        assert_eq!(error.hint(), Some("refresh the instance, then try again"));
        assert_eq!(error.operation_id(), Some(operation));
        assert_eq!(error.rule(), Some("BR-008"));
        assert_eq!(invalid.fields()[0].code(), "too_long");
        assert_eq!(
            error.resource().map(|r| r.kind),
            Some(ResourceKind::Instance)
        );
    }

    #[test]
    #[should_panic(expected = "is not in the catalog entry")]
    #[cfg(debug_assertions)]
    fn a_detail_key_outside_the_catalog_entry_panics_in_debug_builds() {
        let _ = ErrorBody::new(&request::MALFORMED, "bad").with_detail("value", "x");
    }

    #[test]
    fn optional_fields_encode_as_null_and_empty_lists() {
        let error = ErrorBody::new(&request::MALFORMED, "request is not valid json");
        let json = serde_json::to_value(&error).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "code": "request.malformed",
                "category": "invalid_input",
                "message": "request is not valid json",
                "retryable": false,
                "resource": null,
                "fields": [],
                "hint": null,
                "details": [],
                "operationId": null,
                "rule": null,
                "items": [],
            })
        );
    }

    #[test]
    fn missing_optional_fields_decode_as_empty() {
        let error: ErrorBody = serde_json::from_str(
            r#"{"code":"x.y","category":"failed","message":"m","retryable":false}"#,
        )
        .unwrap();
        assert!(error.fields().is_empty() && error.details().is_empty());
        assert!(error.items().is_empty());
        assert_eq!(error.hint(), None);
        assert_eq!(error.spec(), None);
    }

    /// A catalog entry with items, which no spec 0006 code has.
    const ISSUES: ItemSpec = ItemSpec {
        kind: "issue",
        fields: &["path", "code"],
        max: 2,
    };

    fn issue(path: &str) -> ErrorItem {
        ErrorItem::new("issue")
            .with_field("path", path)
            .with_field("code", "required")
    }

    #[test]
    fn items_up_to_the_limit_are_kept_in_order() {
        let error = ErrorBody::new(&request::MALFORMED, "m").push_items(&ISSUES, [issue("a")]);
        let error = error.push_items(&ISSUES, [issue("b")]);

        let paths: Vec<_> = error.items().iter().map(|i| i.field("path")).collect();
        assert_eq!(paths, [Some("a"), Some("b")]);
        assert_eq!(error.items()[0].kind(), "issue");
        assert_eq!(error.detail(OMITTED_DETAIL), None);
    }

    #[test]
    fn items_above_the_limit_are_dropped_and_counted_in_omitted() {
        let error = ErrorBody::new(&request::MALFORMED, "m")
            .push_items(&ISSUES, ["a", "b", "c", "d"].map(issue))
            .push_items(&ISSUES, [issue("e")]);

        assert_eq!(error.items().len(), 2);
        assert_eq!(error.detail(OMITTED_DETAIL), Some("3"));
    }

    #[test]
    #[should_panic(expected = "is not allowed by the catalog entry")]
    #[cfg(debug_assertions)]
    fn an_item_of_another_kind_panics_in_debug_builds() {
        let _ = ErrorBody::new(&request::MALFORMED, "m")
            .push_items(&ISSUES, [ErrorItem::new("reference")]);
    }

    #[test]
    #[should_panic(expected = "is not allowed by the catalog entry")]
    #[cfg(debug_assertions)]
    fn an_item_field_outside_the_catalog_entry_panics_in_debug_builds() {
        let _ = ErrorBody::new(&request::MALFORMED, "m")
            .push_items(&ISSUES, [ErrorItem::new("issue").with_field("value", "x")]);
    }

    #[test]
    #[should_panic(expected = "allows no items")]
    #[cfg(debug_assertions)]
    fn items_on_a_code_without_items_panic_in_debug_builds() {
        let _ = ErrorBody::new(&request::MALFORMED, "m").with_items([issue("a")]);
    }

    #[test]
    fn an_item_of_a_kind_this_build_does_not_know_still_decodes() {
        let error: ErrorBody = serde_json::from_value(serde_json::json!({
            "code": "secret.store_locked",
            "category": "unavailable",
            "message": "m",
            "retryable": false,
            "items": [{ "kind": "hologram", "fields": [{ "key": "x", "value": "1" }] }],
        }))
        .unwrap();
        assert_eq!(error.items()[0].kind(), "hologram");
        assert_eq!(error.items()[0].field("x"), Some("1"));
        assert_eq!(error.code(), "secret.store_locked");
    }

    #[test]
    fn fields_above_the_cap_are_dropped_and_counted_in_omitted() {
        let field = |index: usize| FieldError::new(format!("kinds[{index}]"), FieldCode::TooLong);
        let error = ErrorBody::new(&request::INVALID_PARAMS, "m")
            .with_fields((0..FIELD_ERRORS_MAX + 5).map(field))
            .with_field(field(99))
            .with_fields([field(100), field(101)]);
        assert_eq!(error.fields().len(), FIELD_ERRORS_MAX);
        assert_eq!(error.fields()[0].path(), "kinds[0]");
        assert_eq!(error.fields()[FIELD_ERRORS_MAX - 1].path(), "kinds[31]");
        assert_eq!(error.detail(OMITTED_DETAIL), Some("8"));
    }

    #[test]
    fn fields_up_to_the_cap_add_no_omitted_detail() {
        let error = ErrorBody::new(&request::INVALID_PARAMS, "m").with_fields(
            (0..FIELD_ERRORS_MAX).map(|i| FieldError::new(format!("a[{i}]"), FieldCode::Required)),
        );
        assert_eq!(error.fields().len(), FIELD_ERRORS_MAX);
        assert_eq!(error.detail(OMITTED_DETAIL), None);
    }

    #[test]
    fn every_code_that_carries_fields_lists_the_omitted_detail() {
        for spec in [&request::INVALID_PARAMS, &request::MALFORMED] {
            assert!(spec.details.contains(&OMITTED_DETAIL), "{}", spec.code);
            assert!(spec.items.is_none(), "{}", spec.code);
        }
    }

    #[test]
    fn a_field_path_is_cut_to_256_characters() {
        let field = FieldError::new(format!("settings.{}", "ü".repeat(400)), FieldCode::TooLong);
        assert_eq!(field.path().chars().count(), FIELD_PATH_MAX);
        assert!(field.path().starts_with("settings.üü"));
        assert_eq!(FieldError::new("name", FieldCode::TooLong).path(), "name");
    }

    #[test]
    #[should_panic(expected = "must list `omitted` and allow no items")]
    #[cfg(debug_assertions)]
    fn a_field_on_a_code_without_the_omitted_detail_panics_in_debug_builds() {
        let _ = ErrorBody::new(&request::UNKNOWN_METHOD, "m")
            .with_field(FieldError::new("a", FieldCode::Required));
    }

    #[test]
    fn category_wire_values_match_as_str() {
        for category in ErrorCategory::ALL {
            let json = serde_json::to_value(category).unwrap();
            assert_eq!(json, category.as_str());
        }
    }
}
