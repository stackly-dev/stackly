//! Validation issues: what is wrong with a provider package, where, and
//! under which stable code.

use std::fmt;

/// Most issues one validation keeps. Past it, one final
/// [`IssueCode::TooManyIssues`] issue is added and the rest are dropped.
pub const MAX_ISSUES: usize = 100;

macro_rules! issue_codes {
    ($($(#[$meta:meta])* $variant:ident => $code:literal,)+) => {
        /// The stable kind of a validation [`Issue`].
        ///
        /// The daemon carries the code as the detail of an invalid input
        /// error (spec 0006), so a code never changes once it ships.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        #[non_exhaustive]
        pub enum IssueCode {
            $($(#[$meta])* $variant,)+
        }

        impl IssueCode {
            /// Every code, in declaration order.
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];

            /// Returns the stable snake_case code.
            #[must_use]
            pub const fn as_code(self) -> &'static str {
                match self {
                    $(Self::$variant => $code,)+
                }
            }
        }
    };
}

issue_codes! {
    /// `apiVersion` is not `stackly/v1alpha1`.
    UnsupportedApiVersion => "unsupported_api_version",
    /// `kind` is not `Provider`.
    UnknownKind => "unknown_kind",
    /// A field that the schema does not know.
    UnknownField => "unknown_field",
    /// A required field is absent.
    MissingField => "missing_field",
    /// A value has the wrong type or is outside its closed set or range.
    InvalidValue => "invalid_value",
    /// A key appears twice in one mapping.
    DuplicateKey => "duplicate_key",
    /// The YAML text does not parse.
    YamlSyntax => "yaml_syntax",
    /// The JSON text does not parse.
    JsonSyntax => "json_syntax",
    /// A required package file is absent.
    MissingFile => "missing_file",
    /// An initialize step names a primitive outside the closed set.
    UnknownPrimitive => "unknown_primitive",
    /// A probe type outside the closed set.
    UnknownProbe => "unknown_probe",
    /// A config file format outside the closed set.
    UnknownTemplateFormat => "unknown_template_format",
    /// A reference to a config file that `configFiles` does not declare.
    UnknownConfigFile => "unknown_config_file",
    /// A reference to a secret field that the settings schema does not
    /// declare.
    UnknownSecret => "unknown_secret",
    /// A settings key that the settings schema does not declare.
    UnknownSetting => "unknown_setting",
    /// An interpolation or template token outside the known namespaces.
    UnknownToken => "unknown_token",
    /// A token whose type the position does not accept.
    TokenNotAllowedHere => "token_not_allowed_here",
    /// A `secret.*` token outside stdin, `materializeSecret`, and sensitive
    /// templates.
    SecretNotAllowedHere => "secret_not_allowed_here",
    /// An executable that is not exactly one executable token.
    LiteralExecutable => "literal_executable",
    /// An argument that starts with a free user value, which could become
    /// a new flag.
    ArgumentInjectionRisk => "argument_injection_risk",
    /// A path that leaves the folder its token names.
    PathEscapesScope => "path_escapes_scope",
    /// A folder that `permissions.managedFilesystem` does not list.
    PathNotPermitted => "path_not_permitted",
    /// A name or path component that breaks its charset rule.
    InvalidName => "invalid_name",
    /// A reference to an endpoint that does not exist or cannot serve it.
    UnknownEndpoint => "unknown_endpoint",
    /// An endpoint host that is not loopback.
    EndpointNotLoopback => "endpoint_not_loopback",
    /// A socket name longer than 15 bytes.
    SocketNameTooLong => "socket_name_too_long",
    /// A capability the manifest cannot fulfil.
    CapabilityUnsatisfiable => "capability_unsatisfiable",
    /// Reload settings without the `reload` capability.
    ReloadWithoutCapability => "reload_without_capability",
    /// A timeout outside its range or above the deadline it belongs to.
    TimeoutOutOfRange => "timeout_out_of_range",
    /// A reference to an executable alias that does not exist.
    UnknownExecutable => "unknown_executable",
    /// An executable path that an artifact does not ship.
    ExecutableNotInArtifact => "executable_not_in_artifact",
    /// A template that does not parse.
    TemplateSyntax => "template_syntax",
    /// A template value of the wrong type for its tag.
    TemplateType => "template_type",
    /// A value that the format's escaper cannot write.
    UnrepresentableValue => "unrepresentable_value",
    /// A rendered `yaml` or `json` file that does not parse.
    RenderedOutputInvalid => "rendered_output_invalid",
    /// A rendered file larger than 1 MiB.
    OutputTooLarge => "output_too_large",
    /// A template that no config file uses.
    UnreferencedTemplate => "unreferenced_template",
    /// A package entry that is not allowed.
    UnexpectedEntry => "unexpected_entry",
    /// A symlink in the package.
    SymlinkNotAllowed => "symlink_not_allowed",
    /// A FIFO, socket, device, or folder where a file belongs.
    NotARegularFile => "not_a_regular_file",
    /// A file above its size limit.
    FileTooLarge => "file_too_large",
    /// A file that is not UTF-8 or starts with a byte order mark.
    NotUtf8 => "not_utf8",
    /// A settings schema keyword outside the dialect.
    SettingsSchemaKeyword => "settings_schema_keyword",
    /// A settings schema that breaks a dialect rule.
    SettingsSchemaRule => "settings_schema_rule",
    /// A settings value that the settings schema rejects.
    SettingsValue => "settings_value",
    /// A `versions.json` that breaks a catalog rule.
    VersionsRule => "versions_rule",
    /// A `fixture` artifact outside the `testing` namespace.
    FixtureOutsideTesting => "fixture_outside_testing",
    /// A provider ID whose name differs from the package folder.
    DirectoryNameMismatch => "directory_name_mismatch",
    /// More than [`MAX_ISSUES`] issues; the rest were dropped.
    TooManyIssues => "too_many_issues",
}

impl fmt::Display for IssueCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_code())
    }
}

/// Where in a file an issue is.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Location {
    /// The file as a whole, or the package when the file is empty.
    File,
    /// A field path, such as `lifecycle.start.args[1]`.
    Field(String),
    /// A line and column, both starting at 1.
    Position {
        /// The line.
        line: usize,
        /// The column.
        column: usize,
    },
}

impl fmt::Display for Location {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::File => f.write_str("."),
            Self::Field(path) => f.write_str(path),
            Self::Position { line, column } => write!(f, "{line}:{column}"),
        }
    }
}

/// One problem with a provider package or a settings document.
///
/// `message` is lowercase without a trailing period and never holds a
/// secret or a settings value.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct Issue {
    /// The file: `provider.yaml`, `versions.json`, `settings.schema.json`,
    /// `templates/<name>`, `settings` for a settings document, or the
    /// package entry an entry issue is about.
    pub file: String,
    /// Where in the file.
    pub location: Location,
    /// The stable code.
    pub code: IssueCode,
    /// What is wrong.
    pub message: String,
}

impl Issue {
    /// Creates an issue.
    #[must_use]
    pub fn new(
        file: impl Into<String>,
        location: Location,
        code: IssueCode,
        message: impl Into<String>,
    ) -> Self {
        Self {
            file: file.into(),
            location,
            code,
            message: message.into(),
        }
    }
}

impl fmt::Display for Issue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} {}: {}: {}",
            self.file, self.location, self.code, self.message
        )
    }
}

/// Collects issues up to [`MAX_ISSUES`].
#[derive(Debug, Default)]
pub(crate) struct Issues {
    list: Vec<Issue>,
    full: bool,
    total: usize,
}

impl Issues {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Adds an issue, or the final `too_many_issues` once the list is full.
    pub(crate) fn push(&mut self, issue: Issue) {
        self.total += 1;
        if self.full {
            return;
        }
        if self.list.len() == MAX_ISSUES {
            self.full = true;
            self.list.push(Issue::new(
                "",
                Location::File,
                IssueCode::TooManyIssues,
                format!("more than {MAX_ISSUES} issues, the rest are not reported"),
            ));
            return;
        }
        self.list.push(issue);
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    /// Returns how many issues were pushed, dropped ones included, so a
    /// rule can tell whether it added any.
    pub(crate) fn count(&self) -> usize {
        self.total
    }

    pub(crate) fn into_vec(self) -> Vec<Issue> {
        self.list
    }
}

/// Issues for one file, so rules need not repeat the file name.
pub(crate) struct FileIssues<'a> {
    file: &'a str,
    issues: &'a mut Issues,
}

impl<'a> FileIssues<'a> {
    pub(crate) fn new(file: &'a str, issues: &'a mut Issues) -> Self {
        Self { file, issues }
    }

    /// Adds an issue at a field path.
    pub(crate) fn at(
        &mut self,
        path: impl Into<String>,
        code: IssueCode,
        message: impl Into<String>,
    ) {
        self.issues.push(Issue::new(
            self.file,
            Location::Field(path.into()),
            code,
            message,
        ));
    }

    /// Returns how many issues were pushed so far, in every file.
    pub(crate) fn count(&self) -> usize {
        self.issues.count()
    }

    /// Adds an issue that already has its location.
    pub(crate) fn push(&mut self, location: Location, code: IssueCode, message: impl Into<String>) {
        self.issues
            .push(Issue::new(self.file, location, code, message));
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn codes_are_unique_snake_case() {
        let mut seen = HashSet::new();
        for code in IssueCode::ALL {
            let text = code.as_code();
            assert!(seen.insert(text), "{text} twice");
            assert!(
                text.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'),
                "{text}"
            );
        }
        assert_eq!(IssueCode::ALL.len(), 49);
    }

    #[test]
    fn issues_stop_after_the_limit_with_one_too_many_issues() {
        let mut issues = Issues::new();
        for n in 0..150 {
            issues.push(Issue::new(
                "provider.yaml",
                Location::Field(format!("x[{n}]")),
                IssueCode::InvalidValue,
                "bad",
            ));
        }

        let list = issues.into_vec();

        assert_eq!(list.len(), MAX_ISSUES + 1);
        assert_eq!(list[MAX_ISSUES].code, IssueCode::TooManyIssues);
        assert!(
            list[..MAX_ISSUES]
                .iter()
                .all(|i| i.code == IssueCode::InvalidValue)
        );
    }

    #[test]
    fn an_issue_displays_file_location_code_and_message() {
        let issue = Issue::new(
            "templates/a.tmpl",
            Location::Position { line: 3, column: 7 },
            IssueCode::TemplateSyntax,
            "unclosed tag",
        );

        assert_eq!(
            issue.to_string(),
            "templates/a.tmpl 3:7: template_syntax: unclosed tag"
        );
    }
}
