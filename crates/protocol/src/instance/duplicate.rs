//! The DTOs of `instance.get_duplicate_form` (spec 0024, *Method IPC*): the
//! create form and the create draft of a duplicate, built from the pinned
//! package and the active settings of the source instance.
//!
//! Duplicate is a way to fill the create form, not an operation: the draft
//! goes through `instance.preflight` and `instance.create` of spec 0020
//! unchanged, so this module adds no params or reply to either.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use typeshare::typeshare;
use uuid::Uuid;

use super::{CreateForm, PreflightCreateParams};
use crate::json::Validate;

/// Params of `instance.get_duplicate_form`.
#[typeshare]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GetDuplicateFormParams {
    /// The instance whose settings fill the form, retained ones refused.
    pub instance_id: Uuid,
}

impl Validate for GetDuplicateFormParams {}

/// The instance a duplicate form was built from.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateSource {
    /// The source instance.
    pub instance_id: Uuid,
    /// Its name.
    pub name: String,
}

/// The reference the source uses for one secret field.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateSecret {
    /// The settings field the secret fills.
    pub field: String,
    /// The reference the source uses for the field; absent when it uses
    /// none. It is a reference, never a value.
    #[serde(default)]
    pub source_reference: Option<String>,
}

/// Reply of `instance.get_duplicate_form`.
#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateForm {
    /// The instance the form copies the settings of.
    pub source: DuplicateSource,
    /// The create form of the pinned package and the runtime of the source;
    /// `suggestedName` is the duplicate name.
    pub form: CreateForm,
    /// The draft the form starts with: every port `auto`, every secret
    /// `generate`, the settings of the source, no display name, no start.
    pub draft: PreflightCreateParams,
    /// One entry per secret field, in form order.
    pub secrets: Vec<DuplicateSecret>,
}
