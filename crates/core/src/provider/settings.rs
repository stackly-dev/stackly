//! The settings schema: a restricted JSON Schema 2020-12 dialect decoded
//! into a typed field model (spec 0009, "Settings schema"), and the
//! validation of a settings document against it.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use regex::{Regex, RegexBuilder};
use stackly_domain::{SecretPurpose, SecretRef};

use super::issue::{FileIssues, Issue, IssueCode, Location};
use super::names::{NAME_RULE, SettingName, is_control};
use super::raw::{
    RawItemField, RawItems, RawSettingField, RawSettingsSchema, RawXStackly, join_path,
};

/// The only `$schema` the dialect accepts.
pub(crate) const SCHEMA_DIALECT: &str = "https://json-schema.org/draft/2020-12/schema";
/// Most root fields.
const ROOT_FIELDS_MAX: usize = 64;
/// Most fields of an array item.
const ITEM_FIELDS_MAX: usize = 16;
/// Longest `title`, in characters.
const TITLE_MAX: usize = 64;
/// Longest `description`, in characters.
const DESCRIPTION_MAX: usize = 512;
/// Largest `maxLength`.
const MAX_LENGTH_MAX: u64 = 4096;
/// Most `enum` values.
const ENUM_MAX: usize = 64;
/// Longest `enum` value, in characters.
const ENUM_VALUE_MAX: usize = 128;
/// Longest `pattern`, in characters.
const PATTERN_MAX: usize = 512;
/// Size limit of a compiled pattern, in bytes.
const PATTERN_SIZE_LIMIT: usize = 64 * 1024;
/// Largest `maxItems`.
const MAX_ITEMS_MAX: u64 = 256;
/// Bounds of `minimum` and `maximum`: plus and minus 2^53.
const INTEGER_BOUND: i64 = 1 << 53;
/// Largest settings document, in bytes of compact JSON.
pub const SETTINGS_DOCUMENT_MAX_LEN: usize = 256 * 1024;
/// The `file` of issues about a settings document.
pub(crate) const SETTINGS_FILE: &str = "settings";
/// The secret reference of validation renders, in the daemon's form for the
/// sample instance (spec 0011).
const SAMPLE_SECRET_REF: &str = "local:instance/0192f000-0000-7000-8000-000000000001/sample";

/// When a settings change reaches the running process (VAL-012).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ApplyClass {
    /// Applied by a reload, without a restart.
    Reload,
    /// Applied by a restart.
    Restart,
    /// Fixed once the instance is initialized.
    ImmutableAfterInit,
}

/// Whether a form shows the field by default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum Visibility {
    /// Shown by default.
    #[default]
    Basic,
    /// Shown under advanced settings.
    Advanced,
}

/// The six field classes of VAL-012.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum SettingClass {
    /// Computed by Stackly and shown locked.
    Managed,
    /// Shown under advanced settings.
    Advanced,
    /// A secret, stored as a reference.
    Secret,
    /// Fixed once the instance is initialized.
    ImmutableAfterInit,
    /// Applied by a reload.
    Reloadable,
    /// Applied by a restart.
    RestartRequired,
}

/// A `pattern`, always matched against the whole value.
#[derive(Clone)]
pub struct Pattern {
    source: String,
    regex: Regex,
}

impl Pattern {
    /// Compiles `source` wrapped as `\A(?:source)\z` with a 64 KiB size
    /// limit, so it always matches the whole value in linear time.
    pub(crate) fn compile(source: &str) -> Result<Self, String> {
        if source.chars().count() > PATTERN_MAX {
            return Err(format!("pattern is longer than {PATTERN_MAX} characters"));
        }
        RegexBuilder::new(&format!(r"\A(?:{source})\z"))
            .size_limit(PATTERN_SIZE_LIMIT)
            .build()
            .map(|regex| Self {
                source: source.to_owned(),
                regex,
            })
            .map_err(|_| "pattern does not compile or exceeds the size limit".to_owned())
    }

    /// Returns the pattern as written in the schema.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Returns whether the whole of `text` matches.
    #[must_use]
    pub fn is_match(&self, text: &str) -> bool {
        self.regex.is_match(text)
    }

    /// Returns the regex, for capture groups.
    pub(crate) fn regex(&self) -> &Regex {
        &self.regex
    }
}

impl fmt::Debug for Pattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Pattern({:?})", self.source)
    }
}

impl PartialEq for Pattern {
    fn eq(&self, other: &Self) -> bool {
        self.source == other.source
    }
}

impl Eq for Pattern {}

/// Rules of a text field.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct TextRules {
    /// Fewest characters.
    pub min_length: u64,
    /// Most characters; absent only with `enum_values`.
    pub max_length: Option<u64>,
    /// A pattern the whole value matches.
    pub pattern: Option<Pattern>,
    /// The allowed values.
    pub enum_values: Option<Vec<String>>,
    /// The default.
    pub default: Option<String>,
}

/// Rules of an integer field.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct IntegerRules {
    /// The smallest value.
    pub minimum: i64,
    /// The largest value.
    pub maximum: i64,
    /// The default.
    pub default: Option<i64>,
}

/// Rules of a boolean field.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct BooleanRules {
    /// The default.
    pub default: Option<bool>,
}

/// The type and rules of a scalar field.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ScalarKind {
    /// `type: string`.
    Text(TextRules),
    /// `type: integer`.
    Integer(IntegerRules),
    /// `type: boolean`.
    Boolean(BooleanRules),
}

/// The type and rules of a root field.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum FieldKind {
    /// A string, integer, or boolean.
    Scalar(ScalarKind),
    /// A list of objects.
    Array(ArrayRules),
}

/// Rules of an array field: a list of one level objects.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ArrayRules {
    /// The fields of each item, by name.
    pub items: BTreeMap<SettingName, ItemField>,
    /// Fewest items.
    pub min_items: u64,
    /// Most items.
    pub max_items: u64,
    /// The item field whose value is unique across items.
    pub key: Option<SettingName>,
    /// The default list.
    pub default: Option<Vec<SettingsItem>>,
}

/// One scalar field of an array item.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ItemField {
    /// The field's name.
    pub name: SettingName,
    /// The form label.
    pub title: String,
    /// The help text.
    pub description: Option<String>,
    /// The type and rules.
    pub kind: ScalarKind,
    /// Whether every item must set it.
    pub required: bool,
}

/// One root field of the settings schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingField {
    name: SettingName,
    title: String,
    description: Option<String>,
    kind: FieldKind,
    required: bool,
    apply: Option<ApplyClass>,
    visibility: Visibility,
    secret: bool,
    managed: Option<String>,
}

impl SettingField {
    /// Returns the field's name.
    #[must_use]
    pub const fn name(&self) -> &SettingName {
        &self.name
    }

    /// Returns the form label.
    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Returns the help text.
    #[must_use]
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    /// Returns the type and rules.
    #[must_use]
    pub const fn kind(&self) -> &FieldKind {
        &self.kind
    }

    /// Returns whether a settings document must set it.
    #[must_use]
    pub const fn is_required(&self) -> bool {
        self.required
    }

    /// Returns when a change reaches the process; absent for managed
    /// fields.
    #[must_use]
    pub const fn apply(&self) -> Option<ApplyClass> {
        self.apply
    }

    /// Returns whether a form shows it by default.
    #[must_use]
    pub const fn visibility(&self) -> Visibility {
        self.visibility
    }

    /// Returns whether it holds a secret reference.
    #[must_use]
    pub const fn is_secret(&self) -> bool {
        self.secret
    }

    /// Returns the interpolation that computes a managed field.
    #[must_use]
    pub fn managed(&self) -> Option<&str> {
        self.managed.as_deref()
    }

    /// Returns the field's VAL-012 classes.
    #[must_use]
    pub fn classes(&self) -> Vec<SettingClass> {
        let mut classes = Vec::new();
        if self.managed.is_some() {
            classes.push(SettingClass::Managed);
        }
        if self.visibility == Visibility::Advanced {
            classes.push(SettingClass::Advanced);
        }
        if self.secret {
            classes.push(SettingClass::Secret);
        }
        match self.apply {
            Some(ApplyClass::ImmutableAfterInit) => classes.push(SettingClass::ImmutableAfterInit),
            Some(ApplyClass::Reload) => classes.push(SettingClass::Reloadable),
            Some(ApplyClass::Restart) => classes.push(SettingClass::RestartRequired),
            None => {}
        }
        classes
    }

    /// Whether the value comes straight from the user and could start with
    /// `-`: text without `enum`, or an integer that may be negative.
    pub(crate) fn is_free_user_value(&self) -> bool {
        if self.managed.is_some() {
            return false;
        }
        match &self.kind {
            FieldKind::Scalar(ScalarKind::Text(rules)) => rules.enum_values.is_none(),
            FieldKind::Scalar(ScalarKind::Integer(rules)) => rules.minimum < 0,
            _ => false,
        }
    }
}

/// The typed settings model of one provider package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsSchema {
    title: String,
    description: Option<String>,
    fields: BTreeMap<SettingName, SettingField>,
}

/// One scalar settings value.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ScalarValue {
    /// Text.
    Text(String),
    /// An integer.
    Integer(i64),
    /// A boolean.
    Boolean(bool),
}

impl ScalarValue {
    fn to_json(&self) -> serde_json::Value {
        match self {
            Self::Text(text) => text.clone().into(),
            Self::Integer(value) => (*value).into(),
            Self::Boolean(value) => (*value).into(),
        }
    }
}

/// One item of an array field.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SettingsItem(BTreeMap<SettingName, ScalarValue>);

impl SettingsItem {
    /// Returns one field's value.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&ScalarValue> {
        self.0
            .iter()
            .find(|(key, _)| key.as_str() == name)
            .map(|(_, value)| value)
    }

    fn to_json(&self) -> serde_json::Value {
        self.0
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_json()))
            .collect::<serde_json::Map<_, _>>()
            .into()
    }
}

/// One settings value.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SettingValue {
    /// A scalar.
    Scalar(ScalarValue),
    /// A list of items.
    Array(Vec<SettingsItem>),
    /// A reference to a secret, never its value.
    Secret(SecretRef),
}

/// A settings document that passed [`SettingsSchema::validate`], with
/// defaults filled in. Managed fields are absent: plans compute them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SettingsValues(BTreeMap<SettingName, SettingValue>);

impl SettingsValues {
    /// Returns one field's value.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&SettingValue> {
        self.0
            .iter()
            .find(|(key, _)| key.as_str() == name)
            .map(|(_, value)| value)
    }

    /// Returns the JSON object the registry stores (spec 0005): one key
    /// per field, secrets as their reference text.
    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        self.0
            .iter()
            .map(|(key, value)| {
                let json = match value {
                    SettingValue::Scalar(scalar) => scalar.to_json(),
                    SettingValue::Array(items) => items.iter().map(SettingsItem::to_json).collect(),
                    SettingValue::Secret(reference) => reference.as_str().into(),
                };
                (key.to_string(), json)
            })
            .collect::<serde_json::Map<_, _>>()
            .into()
    }
}

/// Error returned when a settings document does not match its schema.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("settings have {} issues", issues.len())]
#[non_exhaustive]
pub struct SettingsError {
    /// Every problem, with file `settings` and the field path as location.
    /// Codes are `settings_value`, or `unknown_setting` for a key the
    /// schema does not declare. Messages never hold the value.
    pub issues: Vec<Issue>,
}

impl SettingsSchema {
    /// Returns the schema's title.
    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Returns the schema's description.
    #[must_use]
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    /// Returns every field, in name order.
    pub fn fields(&self) -> impl Iterator<Item = &SettingField> {
        self.fields.values()
    }

    /// Returns one field by name.
    #[must_use]
    pub fn field(&self, name: &str) -> Option<&SettingField> {
        self.fields
            .iter()
            .find(|(key, _)| key.as_str() == name)
            .map(|(_, field)| field)
    }

    /// Validates a settings document and fills in defaults.
    ///
    /// The document is an object of at most 256 KiB. Keys the schema does
    /// not declare and values for managed fields are rejected; types,
    /// bounds, `enum`, and `pattern` are checked; integers must be whole
    /// JSON literals (not `5.0` or `1e3`); text holds no control
    /// characters; item keys marked `key` are unique; secret values parse
    /// as a [`SecretRef`]. Afterwards every field that is not managed has
    /// a value.
    ///
    /// # Errors
    ///
    /// Returns a [`SettingsError`] with every problem found.
    pub fn validate(&self, document: &serde_json::Value) -> Result<SettingsValues, SettingsError> {
        let mut problems = Problems::default();
        let size = serde_json::to_string(document).map_or(usize::MAX, |text| text.len());
        let object = match document {
            _ if size > SETTINGS_DOCUMENT_MAX_LEN => {
                problems.value("", "settings are larger than 256 KiB");
                None
            }
            serde_json::Value::Object(object) => Some(object),
            _ => {
                problems.value("", "settings must be an object");
                None
            }
        };
        let mut values = BTreeMap::new();
        if let Some(object) = object {
            for key in object.keys() {
                match self.field(key) {
                    None => {
                        problems.unknown(key, format!("the settings schema has no field {key}"))
                    }
                    Some(field) if field.managed.is_some() => {
                        problems.value(
                            key,
                            "managed fields are computed by Stackly and cannot be set",
                        );
                    }
                    Some(_) => {}
                }
            }
            for field in self.fields.values().filter(|field| field.managed.is_none()) {
                let path = field.name.as_str();
                if let Some(value) = object.get(path) {
                    if let Some(value) = check_field(field, value, path, &mut problems) {
                        values.insert(field.name.clone(), value);
                    }
                } else if let Some(default) = field.default_value() {
                    values.insert(field.name.clone(), default);
                } else {
                    problems.value(path, "required field is missing");
                }
            }
        }
        if problems.list.is_empty() {
            Ok(SettingsValues(values))
        } else {
            Err(SettingsError {
                issues: problems.into_issues(SETTINGS_FILE, IssueCode::SettingsValue),
            })
        }
    }

    /// Builds a settings document for validation renders: defaults, the
    /// first enum value or a minimal text otherwise, a sample secret
    /// reference, and for arrays either no items or one sample item.
    pub(crate) fn sample_values(&self, with_items: bool) -> SettingsValues {
        let mut values = BTreeMap::new();
        for field in self.fields.values().filter(|field| field.managed.is_none()) {
            let value = if field.secret {
                SettingValue::Secret(
                    SAMPLE_SECRET_REF
                        .parse()
                        .expect("the sample secret reference is a valid secret reference"),
                )
            } else {
                match &field.kind {
                    FieldKind::Scalar(kind) => SettingValue::Scalar(sample_scalar(kind)),
                    FieldKind::Array(rules) => {
                        let items = if with_items {
                            let item = rules
                                .items
                                .iter()
                                .map(|(name, item)| (name.clone(), sample_scalar(&item.kind)))
                                .collect();
                            vec![SettingsItem(item)]
                        } else {
                            Vec::new()
                        };
                        SettingValue::Array(items)
                    }
                }
            };
            values.insert(field.name.clone(), value);
        }
        SettingsValues(values)
    }
}

impl SettingField {
    fn default_value(&self) -> Option<SettingValue> {
        match &self.kind {
            FieldKind::Scalar(kind) => scalar_default(kind).map(SettingValue::Scalar),
            FieldKind::Array(rules) => rules.default.clone().map(SettingValue::Array),
        }
    }
}

fn scalar_default(kind: &ScalarKind) -> Option<ScalarValue> {
    match kind {
        ScalarKind::Text(rules) => rules.default.clone().map(ScalarValue::Text),
        ScalarKind::Integer(rules) => rules.default.map(ScalarValue::Integer),
        ScalarKind::Boolean(rules) => rules.default.map(ScalarValue::Boolean),
    }
}

fn sample_scalar(kind: &ScalarKind) -> ScalarValue {
    scalar_default(kind).unwrap_or_else(|| match kind {
        ScalarKind::Text(rules) => ScalarValue::Text(match &rules.enum_values {
            Some(values) => values.first().cloned().unwrap_or_default(),
            None => "x".repeat(usize::try_from(rules.min_length.max(1)).unwrap_or(1)),
        }),
        ScalarKind::Integer(rules) => ScalarValue::Integer(0.clamp(rules.minimum, rules.maximum)),
        ScalarKind::Boolean(_) => ScalarValue::Boolean(false),
    })
}

/// Problems with a value: (path, message, unknown key).
#[derive(Default)]
struct Problems {
    list: Vec<(String, String, bool)>,
}

impl Problems {
    fn value(&mut self, path: &str, message: impl Into<String>) {
        self.list.push((path.to_owned(), message.into(), false));
    }

    fn unknown(&mut self, path: &str, message: impl Into<String>) {
        self.list.push((path.to_owned(), message.into(), true));
    }

    fn into_issues(self, file: &str, code: IssueCode) -> Vec<Issue> {
        self.list
            .into_iter()
            .map(|(path, message, unknown)| {
                let path = if path.is_empty() {
                    ".".to_owned()
                } else {
                    path
                };
                let code = if unknown {
                    IssueCode::UnknownSetting
                } else {
                    code
                };
                Issue::new(file, Location::Field(path), code, message)
            })
            .collect()
    }
}

fn check_field(
    field: &SettingField,
    value: &serde_json::Value,
    path: &str,
    problems: &mut Problems,
) -> Option<SettingValue> {
    if field.secret {
        return match value.as_str().map(str::parse::<SecretRef>) {
            Some(Ok(reference)) => Some(SettingValue::Secret(reference)),
            _ => {
                problems.value(path, "a secret field holds a secret reference");
                None
            }
        };
    }
    match &field.kind {
        FieldKind::Scalar(kind) => {
            check_scalar(kind, value, path, problems).map(SettingValue::Scalar)
        }
        FieldKind::Array(rules) => {
            check_array(rules, value, path, problems).map(SettingValue::Array)
        }
    }
}

fn check_scalar(
    kind: &ScalarKind,
    value: &serde_json::Value,
    path: &str,
    problems: &mut Problems,
) -> Option<ScalarValue> {
    match kind {
        ScalarKind::Text(rules) => {
            let Some(text) = value.as_str() else {
                problems.value(path, "must be text");
                return None;
            };
            let before = problems.list.len();
            if text.chars().any(is_control) {
                problems.value(path, "text cannot hold control characters");
            }
            let len = text.chars().count() as u64;
            if len < rules.min_length || rules.max_length.is_some_and(|max| len > max) {
                problems.value(
                    path,
                    match rules.max_length {
                        Some(max) => format!("must be {} to {max} characters", rules.min_length),
                        None => format!("must be at least {} characters", rules.min_length),
                    },
                );
            }
            if let Some(values) = &rules.enum_values
                && !values.iter().any(|allowed| allowed == text)
            {
                problems.value(path, "must be one of the allowed values");
            }
            if let Some(pattern) = &rules.pattern
                && !pattern.is_match(text)
            {
                problems.value(path, "does not match the required pattern");
            }
            (problems.list.len() == before).then(|| ScalarValue::Text(text.to_owned()))
        }
        ScalarKind::Integer(rules) => {
            let number = match value {
                serde_json::Value::Number(number) if !number.is_f64() => number.as_i64(),
                _ => {
                    problems.value(
                        path,
                        "must be a whole number written without a fraction or exponent",
                    );
                    return None;
                }
            };
            match number {
                Some(number) if (rules.minimum..=rules.maximum).contains(&number) => {
                    Some(ScalarValue::Integer(number))
                }
                _ => {
                    problems.value(
                        path,
                        format!("must be between {} and {}", rules.minimum, rules.maximum),
                    );
                    None
                }
            }
        }
        ScalarKind::Boolean(_) => match value.as_bool() {
            Some(flag) => Some(ScalarValue::Boolean(flag)),
            None => {
                problems.value(path, "must be true or false");
                None
            }
        },
    }
}

fn check_array(
    rules: &ArrayRules,
    value: &serde_json::Value,
    path: &str,
    problems: &mut Problems,
) -> Option<Vec<SettingsItem>> {
    let Some(list) = value.as_array() else {
        problems.value(path, "must be a list");
        return None;
    };
    let before = problems.list.len();
    let len = list.len() as u64;
    if len < rules.min_items || len > rules.max_items {
        problems.value(
            path,
            format!("must hold {} to {} items", rules.min_items, rules.max_items),
        );
    }
    let mut items = Vec::new();
    let mut keys = BTreeSet::new();
    for (index, item) in list.iter().enumerate() {
        let item_path = format!("{path}[{index}]");
        let Some(object) = item.as_object() else {
            problems.value(&item_path, "must be an object");
            continue;
        };
        for key in object.keys() {
            if !rules.items.keys().any(|name| name.as_str() == key) {
                problems.unknown(
                    &join_path(&item_path, key),
                    format!("items have no field {key}"),
                );
            }
        }
        let mut values = BTreeMap::new();
        for (name, field) in &rules.items {
            let field_path = join_path(&item_path, name.as_str());
            match object.get(name.as_str()) {
                Some(value) => {
                    if let Some(value) = check_scalar(&field.kind, value, &field_path, problems) {
                        values.insert(name.clone(), value);
                    }
                }
                None => match scalar_default(&field.kind) {
                    Some(default) => {
                        values.insert(name.clone(), default);
                    }
                    None => problems.value(&field_path, "required field is missing"),
                },
            }
        }
        if let Some(key) = &rules.key
            && let Some(value) = values.get(key)
        {
            let text = value.to_json().to_string();
            if !keys.insert(text) {
                problems.value(
                    &join_path(&item_path, key.as_str()),
                    "must be unique across items",
                );
            }
        }
        items.push(SettingsItem(values));
    }
    (problems.list.len() == before).then_some(items)
}

/// Turns the raw schema into the typed model, reporting every dialect
/// rule it breaks.
pub(crate) fn parse_schema(
    raw: RawSettingsSchema,
    issues: &mut FileIssues<'_>,
) -> Option<SettingsSchema> {
    let start = issues_len(issues);
    let rule = IssueCode::SettingsSchemaRule;
    if raw.schema != SCHEMA_DIALECT {
        issues.at("$schema", rule, format!("$schema must be {SCHEMA_DIALECT}"));
    }
    if raw.kind != "object" {
        issues.at("type", rule, "the root type must be object");
    }
    if raw.additional_properties {
        issues.at(
            "additionalProperties",
            rule,
            "additionalProperties must be false",
        );
    }
    check_title(&raw.title, "title", issues);
    check_description(raw.description.as_deref(), "description", issues);
    if raw.properties.len() > ROOT_FIELDS_MAX {
        issues.at(
            "properties",
            rule,
            format!("at most {ROOT_FIELDS_MAX} fields"),
        );
    }
    let required = check_required(&raw.required, raw.properties.keys(), "required", issues);
    let mut fields = BTreeMap::new();
    for (name, field) in raw.properties {
        let path = format!("properties.{name}");
        let Ok(setting) = name.parse::<SettingName>() else {
            issues.at(
                &path,
                IssueCode::InvalidName,
                format!("field names {NAME_RULE}"),
            );
            continue;
        };
        if let Some(field) = parse_field(
            setting.clone(),
            field,
            required.contains(&name),
            &path,
            issues,
        ) {
            fields.insert(setting, field);
        }
    }
    check_secret_purposes(&fields, issues);
    (issues_len(issues) == start).then(|| SettingsSchema {
        title: raw.title,
        description: raw.description,
        fields,
    })
}

/// Rejects two secret fields whose purposes are equal, since the daemon
/// names each secret of an instance `local:instance/<id>/<purpose>`
/// (spec 0011).
fn check_secret_purposes(
    fields: &BTreeMap<SettingName, SettingField>,
    issues: &mut FileIssues<'_>,
) {
    let mut seen: BTreeMap<SecretPurpose, &SettingName> = BTreeMap::new();
    for field in fields.values().filter(|field| field.is_secret()) {
        // A setting name has at most 32 characters, so its purpose has at
        // most 48 bytes and always derives.
        let Ok(purpose) = SecretPurpose::from_field_name(field.name().as_str()) else {
            continue;
        };
        match seen.get(&purpose) {
            Some(first) => issues.at(
                format!("properties.{}", field.name()),
                IssueCode::SecretPurposeCollision,
                format!(
                    "secret fields {first} and {} both have the purpose {purpose}, so their secret references would be the same",
                    field.name()
                ),
            ),
            None => {
                seen.insert(purpose, field.name());
            }
        }
    }
}

/// How many issues were reported so far, to tell whether a part was clean.
fn issues_len(issues: &FileIssues<'_>) -> usize {
    issues.count()
}

fn check_title(title: &str, path: &str, issues: &mut FileIssues<'_>) {
    let len = title.chars().count();
    if !(1..=TITLE_MAX).contains(&len) || title.chars().any(is_control) {
        issues.at(
            path,
            IssueCode::SettingsSchemaRule,
            format!("title must be 1 to {TITLE_MAX} characters without control characters"),
        );
    }
}

fn check_description(description: Option<&str>, path: &str, issues: &mut FileIssues<'_>) {
    if let Some(text) = description
        && (text.chars().count() > DESCRIPTION_MAX || text.chars().any(is_control))
    {
        issues.at(path, IssueCode::SettingsSchemaRule, format!("description must be at most {DESCRIPTION_MAX} characters without control characters"));
    }
}

fn check_required<'a>(
    required: &[String],
    names: impl Iterator<Item = &'a String>,
    path: &str,
    issues: &mut FileIssues<'_>,
) -> BTreeSet<String> {
    let names: BTreeSet<&String> = names.collect();
    let mut set = BTreeSet::new();
    for (index, name) in required.iter().enumerate() {
        if !names.contains(name) {
            issues.at(
                format!("{path}[{index}]"),
                IssueCode::SettingsSchemaRule,
                format!("required names a field that does not exist: {name}"),
            );
        } else if !set.insert(name.clone()) {
            issues.at(
                format!("{path}[{index}]"),
                IssueCode::SettingsSchemaRule,
                "required lists a field twice",
            );
        }
    }
    set
}

/// The keywords a field sets, to check which apply to its type.
struct Keywords {
    enum_values: bool,
    min_length: bool,
    max_length: bool,
    pattern: bool,
    minimum: bool,
    maximum: bool,
    items: bool,
    min_items: bool,
    max_items: bool,
}

fn reject_keywords(keywords: &Keywords, kind: &str, path: &str, issues: &mut FileIssues<'_>) {
    let text = matches!(kind, "string");
    let integer = matches!(kind, "integer");
    let array = matches!(kind, "array");
    let checks = [
        (keywords.enum_values, text, "enum", "string"),
        (keywords.min_length, text, "minLength", "string"),
        (keywords.max_length, text, "maxLength", "string"),
        (keywords.pattern, text, "pattern", "string"),
        (keywords.minimum, integer, "minimum", "integer"),
        (keywords.maximum, integer, "maximum", "integer"),
        (keywords.items, array, "items", "array"),
        (keywords.min_items, array, "minItems", "array"),
        (keywords.max_items, array, "maxItems", "array"),
    ];
    for (set, allowed, keyword, owner) in checks {
        if set && !allowed {
            issues.at(
                join_path(path, keyword),
                IssueCode::SettingsSchemaRule,
                format!("{keyword} applies only to {owner} fields"),
            );
        }
    }
}

/// The scalar keywords shared by root and item fields.
struct ScalarParts<'a> {
    kind: &'a str,
    default: Option<&'a serde_json::Value>,
    enum_values: Option<&'a Vec<String>>,
    min_length: Option<u64>,
    max_length: Option<u64>,
    pattern: Option<&'a str>,
    minimum: Option<i64>,
    maximum: Option<i64>,
}

fn parse_scalar(
    parts: &ScalarParts<'_>,
    path: &str,
    issues: &mut FileIssues<'_>,
) -> Option<ScalarKind> {
    let rule = IssueCode::SettingsSchemaRule;
    let start = issues.count();
    let kind = match parts.kind {
        "string" => {
            if let Some(values) = parts.enum_values {
                let unique: BTreeSet<&String> = values.iter().collect();
                if !(1..=ENUM_MAX).contains(&values.len()) || unique.len() != values.len() {
                    issues.at(
                        join_path(path, "enum"),
                        rule,
                        format!("enum must hold 1 to {ENUM_MAX} unique values"),
                    );
                }
                for (index, value) in values.iter().enumerate() {
                    if value.chars().count() > ENUM_VALUE_MAX
                        || value.starts_with('-')
                        || value.chars().any(is_control)
                    {
                        issues.at(
                            format!("{}[{index}]", join_path(path, "enum")),
                            rule,
                            format!("enum values are at most {ENUM_VALUE_MAX} characters, never start with -, and hold no control characters"),
                        );
                    }
                }
            }
            match parts.max_length {
                None if parts.enum_values.is_none() => issues.at(
                    join_path(path, "maxLength"),
                    rule,
                    "maxLength is required unless enum is set",
                ),
                Some(max) if max > MAX_LENGTH_MAX => issues.at(
                    join_path(path, "maxLength"),
                    rule,
                    format!("maxLength is at most {MAX_LENGTH_MAX}"),
                ),
                _ => {}
            }
            let min_length = parts.min_length.unwrap_or(0);
            if parts.max_length.is_some_and(|max| min_length > max) {
                issues.at(
                    join_path(path, "minLength"),
                    rule,
                    "minLength cannot exceed maxLength",
                );
            }
            let pattern = parts
                .pattern
                .and_then(|source| match Pattern::compile(source) {
                    Ok(pattern) => Some(pattern),
                    Err(message) => {
                        issues.at(join_path(path, "pattern"), rule, message);
                        None
                    }
                });
            ScalarKind::Text(TextRules {
                min_length,
                max_length: parts.max_length,
                pattern,
                enum_values: parts.enum_values.cloned(),
                default: None,
            })
        }
        "integer" => {
            let (Some(minimum), Some(maximum)) = (parts.minimum, parts.maximum) else {
                issues.at(path, rule, "integer fields need both minimum and maximum");
                return None;
            };
            let bound = -INTEGER_BOUND..=INTEGER_BOUND;
            if !bound.contains(&minimum) || !bound.contains(&maximum) || minimum > maximum {
                issues.at(
                    path,
                    rule,
                    "minimum and maximum must lie within plus and minus 2^53, minimum first",
                );
            }
            ScalarKind::Integer(IntegerRules {
                minimum,
                maximum,
                default: None,
            })
        }
        "boolean" => ScalarKind::Boolean(BooleanRules { default: None }),
        _ => {
            issues.at(
                join_path(path, "type"),
                rule,
                "type must be string, integer, boolean, or array",
            );
            return None;
        }
    };
    if issues.count() != start {
        return None;
    }
    let Some(default) = parts.default else {
        return Some(kind);
    };
    let mut problems = Problems::default();
    let value = check_scalar(&kind, default, "", &mut problems);
    for (_, message, _) in problems.list {
        issues.at(
            join_path(path, "default"),
            rule,
            format!("default breaks its own rule: {message}"),
        );
    }
    let value = value?;
    Some(match (kind, value) {
        (ScalarKind::Text(rules), ScalarValue::Text(text)) => ScalarKind::Text(TextRules {
            default: Some(text),
            ..rules
        }),
        (ScalarKind::Integer(rules), ScalarValue::Integer(number)) => {
            ScalarKind::Integer(IntegerRules {
                default: Some(number),
                ..rules
            })
        }
        (ScalarKind::Boolean(_), ScalarValue::Boolean(flag)) => ScalarKind::Boolean(BooleanRules {
            default: Some(flag),
        }),
        (kind, _) => kind,
    })
}

fn parse_field(
    name: SettingName,
    mut raw: RawSettingField,
    required: bool,
    path: &str,
    issues: &mut FileIssues<'_>,
) -> Option<SettingField> {
    let rule = IssueCode::SettingsSchemaRule;
    let start = issues.count();
    check_title(&raw.title, &join_path(path, "title"), issues);
    check_description(
        raw.description.as_deref(),
        &join_path(path, "description"),
        issues,
    );
    let keywords = Keywords {
        enum_values: raw.enum_values.is_some(),
        min_length: raw.min_length.is_some(),
        max_length: raw.max_length.is_some(),
        pattern: raw.pattern.is_some(),
        minimum: raw.minimum.is_some(),
        maximum: raw.maximum.is_some(),
        items: raw.items.is_some(),
        min_items: raw.min_items.is_some(),
        max_items: raw.max_items.is_some(),
    };
    reject_keywords(&keywords, &raw.kind, path, issues);
    let x = raw.x_stackly.take().unwrap_or(RawXStackly {
        apply: None,
        visibility: None,
        secret: None,
        managed: None,
        key: None,
    });
    let x_path = join_path(path, "x-stackly");
    let apply = match x.apply.as_deref() {
        None => None,
        Some("reload") => Some(ApplyClass::Reload),
        Some("restart") => Some(ApplyClass::Restart),
        Some("immutableAfterInit") => Some(ApplyClass::ImmutableAfterInit),
        Some(_) => {
            issues.at(
                join_path(&x_path, "apply"),
                rule,
                "apply must be reload, restart, or immutableAfterInit",
            );
            None
        }
    };
    let visibility = match x.visibility.as_deref() {
        None | Some("basic") => Visibility::Basic,
        Some("advanced") => Visibility::Advanced,
        Some(_) => {
            issues.at(
                join_path(&x_path, "visibility"),
                rule,
                "visibility must be basic or advanced",
            );
            Visibility::Basic
        }
    };
    let secret = x.secret.unwrap_or(false);
    let managed = x.managed;
    if managed.is_none() && x.apply.is_none() {
        issues.at(
            join_path(&x_path, "apply"),
            rule,
            "x-stackly.apply is required unless the field is managed",
        );
    }
    if secret {
        if raw.kind != "string" {
            issues.at(
                join_path(&x_path, "secret"),
                rule,
                "only string fields can be secret",
            );
        }
        if !required {
            issues.at(path, rule, "secret fields must be required");
        }
        for (set, keyword) in [
            (raw.default.is_some(), "default"),
            (raw.enum_values.is_some(), "enum"),
            (raw.pattern.is_some(), "pattern"),
        ] {
            if set {
                issues.at(
                    join_path(path, keyword),
                    rule,
                    format!("secret fields cannot have {keyword}"),
                );
            }
        }
        if managed.is_some() {
            issues.at(
                join_path(&x_path, "managed"),
                rule,
                "a secret field cannot be managed",
            );
        }
    }
    if managed.is_some() {
        if required {
            issues.at(path, rule, "managed fields cannot be required");
        }
        if raw.default.is_some() {
            issues.at(
                join_path(path, "default"),
                rule,
                "managed fields cannot have a default",
            );
        }
        if !matches!(raw.kind.as_str(), "string" | "integer") {
            issues.at(
                join_path(path, "type"),
                rule,
                "managed fields are strings or integers",
            );
        }
    } else if !secret && !required && raw.default.is_none() {
        issues.at(path, rule, "a field must be required or have a default");
    }
    if x.key.is_some() && raw.kind != "array" {
        issues.at(
            join_path(&x_path, "key"),
            rule,
            "key applies only to array fields",
        );
    }
    let kind = if raw.kind == "array" {
        parse_array(&raw, x.key.as_deref(), path, issues).map(FieldKind::Array)
    } else {
        let parts = ScalarParts {
            kind: &raw.kind,
            default: raw.default.as_ref(),
            enum_values: raw.enum_values.as_ref(),
            min_length: raw.min_length,
            max_length: raw.max_length,
            pattern: raw.pattern.as_deref(),
            minimum: raw.minimum,
            maximum: raw.maximum,
        };
        parse_scalar(&parts, path, issues).map(FieldKind::Scalar)
    };
    let kind = kind?;
    (issues.count() == start).then_some(SettingField {
        name,
        title: raw.title,
        description: raw.description,
        kind,
        required,
        apply,
        visibility,
        secret,
        managed,
    })
}

fn parse_array(
    raw: &RawSettingField,
    key: Option<&str>,
    path: &str,
    issues: &mut FileIssues<'_>,
) -> Option<ArrayRules> {
    let rule = IssueCode::SettingsSchemaRule;
    let start = issues.count();
    let Some(items) = &raw.items else {
        issues.at(join_path(path, "items"), rule, "array fields need items");
        return None;
    };
    let max_items = match raw.max_items {
        Some(max) if max <= MAX_ITEMS_MAX => max,
        Some(_) => {
            issues.at(
                join_path(path, "maxItems"),
                rule,
                format!("maxItems is at most {MAX_ITEMS_MAX}"),
            );
            return None;
        }
        None => {
            issues.at(
                join_path(path, "maxItems"),
                rule,
                "array fields need maxItems",
            );
            return None;
        }
    };
    let min_items = raw.min_items.unwrap_or(0);
    if min_items > max_items {
        issues.at(
            join_path(path, "minItems"),
            rule,
            "minItems cannot exceed maxItems",
        );
    }
    let item_fields = parse_items(items, &join_path(path, "items"), issues);
    let key = key.and_then(
        |key| match item_fields.iter().find(|(name, _)| name.as_str() == key) {
            Some((name, field)) if !matches!(field.kind, ScalarKind::Boolean(_)) => {
                Some(name.clone())
            }
            _ => {
                issues.at(
                    join_path(path, "x-stackly.key"),
                    rule,
                    "key must name a text or integer item field",
                );
                None
            }
        },
    );
    if issues.count() != start {
        return None;
    }
    let mut rules = ArrayRules {
        items: item_fields,
        min_items,
        max_items,
        key,
        default: None,
    };
    if let Some(default) = &raw.default {
        let mut problems = Problems::default();
        let value = check_array(&rules, default, "default", &mut problems);
        for (at, message, _) in problems.list {
            issues.at(
                join_path(path, &at),
                rule,
                format!("default breaks its own rule: {message}"),
            );
        }
        rules.default = Some(value?);
    }
    Some(rules)
}

fn parse_items(
    raw: &RawItems,
    path: &str,
    issues: &mut FileIssues<'_>,
) -> BTreeMap<SettingName, ItemField> {
    let rule = IssueCode::SettingsSchemaRule;
    if raw.kind != "object" {
        issues.at(
            join_path(path, "type"),
            rule,
            "items must be of type object",
        );
    }
    if raw.additional_properties {
        issues.at(
            join_path(path, "additionalProperties"),
            rule,
            "additionalProperties must be false",
        );
    }
    if !(1..=ITEM_FIELDS_MAX).contains(&raw.properties.len()) {
        issues.at(
            join_path(path, "properties"),
            rule,
            format!("items hold 1 to {ITEM_FIELDS_MAX} fields"),
        );
    }
    let required = check_required(
        &raw.required,
        raw.properties.keys(),
        &join_path(path, "required"),
        issues,
    );
    let mut fields = BTreeMap::new();
    for (name, field) in &raw.properties {
        let field_path = join_path(&join_path(path, "properties"), name);
        let Ok(setting) = name.parse::<SettingName>() else {
            issues.at(
                &field_path,
                IssueCode::InvalidName,
                format!("field names {NAME_RULE}"),
            );
            continue;
        };
        if let Some(item) = parse_item_field(
            setting.clone(),
            field,
            required.contains(name),
            &field_path,
            issues,
        ) {
            fields.insert(setting, item);
        }
    }
    fields
}

fn parse_item_field(
    name: SettingName,
    raw: &RawItemField,
    required: bool,
    path: &str,
    issues: &mut FileIssues<'_>,
) -> Option<ItemField> {
    let start = issues.count();
    check_title(&raw.title, &join_path(path, "title"), issues);
    check_description(
        raw.description.as_deref(),
        &join_path(path, "description"),
        issues,
    );
    if raw.kind == "array" {
        issues.at(
            join_path(path, "type"),
            IssueCode::SettingsSchemaRule,
            "items hold only string, integer, and boolean fields",
        );
        return None;
    }
    let keywords = Keywords {
        enum_values: raw.enum_values.is_some(),
        min_length: raw.min_length.is_some(),
        max_length: raw.max_length.is_some(),
        pattern: raw.pattern.is_some(),
        minimum: raw.minimum.is_some(),
        maximum: raw.maximum.is_some(),
        items: false,
        min_items: false,
        max_items: false,
    };
    reject_keywords(&keywords, &raw.kind, path, issues);
    if !required && raw.default.is_none() {
        issues.at(
            path,
            IssueCode::SettingsSchemaRule,
            "an item field must be required or have a default",
        );
    }
    let parts = ScalarParts {
        kind: &raw.kind,
        default: raw.default.as_ref(),
        enum_values: raw.enum_values.as_ref(),
        min_length: raw.min_length,
        max_length: raw.max_length,
        pattern: raw.pattern.as_deref(),
        minimum: raw.minimum,
        maximum: raw.maximum,
    };
    let kind = parse_scalar(&parts, path, issues)?;
    (issues.count() == start).then(|| ItemField {
        name,
        title: raw.title.clone(),
        description: raw.description.clone(),
        kind,
        required,
    })
}
