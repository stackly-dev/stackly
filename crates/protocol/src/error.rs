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

use crate::codes::{self, ErrorSpec};
use crate::json::FieldCode;
use crate::wire::ResourceRef;

/// Longest `message`, in characters; [`ErrorBody::new`] truncates longer
/// ones on a character boundary.
pub const MESSAGE_MAX: usize = 200;

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
/// `code`, `category`, `message`, and `details` are frozen across majors, so
/// peers of any major can always exchange `protocol.major_mismatch`.
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
        }
    }

    /// Adds the resource concerned.
    #[must_use]
    pub fn with_resource(mut self, resource: ResourceRef) -> Self {
        self.resource = Some(resource);
        self
    }

    /// Adds one failed params field.
    #[must_use]
    pub fn with_field(mut self, field: FieldError) -> Self {
        self.fields.push(field);
        self
    }

    /// Adds failed params fields.
    #[must_use]
    pub fn with_fields(mut self, fields: impl IntoIterator<Item = FieldError>) -> Self {
        self.fields.extend(fields);
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
}

/// One params field that failed to decode or validate.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FieldError {
    /// Where the field sits, relative to the params, such as `name`,
    /// `since.sequence`, or `kinds[3]`.
    path: String,
    /// A code from the fixed set in [`FieldCode`].
    code: String,
    /// The fixed text of the code; never the input value.
    message: String,
}

impl FieldError {
    /// A failed field at `path` with the fixed code and text of `code`.
    #[must_use]
    pub fn new(path: impl Into<String>, code: FieldCode) -> Self {
        Self {
            path: path.into(),
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
            .with_rule("BR-008")
            .with_field(FieldError::new("name", FieldCode::TooLong));
        assert_eq!(error.detail("expected"), Some("3"));
        assert_eq!(error.detail("actual"), Some("4"));
        assert_eq!(error.detail("missing"), None);
        assert_eq!(error.hint(), Some("refresh the instance, then try again"));
        assert_eq!(error.operation_id(), Some(operation));
        assert_eq!(error.rule(), Some("BR-008"));
        assert_eq!(error.fields()[0].code(), "too_long");
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
        assert_eq!(error.hint(), None);
        assert_eq!(error.spec(), None);
    }

    #[test]
    fn category_wire_values_match_as_str() {
        for category in ErrorCategory::ALL {
            let json = serde_json::to_value(category).unwrap();
            assert_eq!(json, category.as_str());
        }
    }
}
