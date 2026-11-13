//! Findings of an environment manifest check (spec 0044, *Aturan
//! validasi*): a stable code, where, and a message built from a fixed
//! template, never from a value in the file.

use std::fmt;

use stackly_domain::MemberAlias;

/// Most findings one validation returns; the rest are counted in
/// [`Findings::omitted`].
pub const MAX_FINDINGS: usize = 100;

/// How serious a finding is. Every manifest code is an error; the local
/// overrides of spec 0045 have one warning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum Severity {
    /// The manifest cannot be planned, imported, or brought up.
    Error,
    /// A note that blocks nothing.
    Warning,
}

impl Severity {
    /// Returns the stable code.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
        }
    }
}

macro_rules! finding_codes {
    ($($(#[$meta:meta])* $variant:ident => $code:literal,)+) => {
        /// The stable kind of a [`Finding`]. A code never changes once it
        /// ships.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        #[non_exhaustive]
        pub enum FindingCode {
            $($(#[$meta])* $variant,)+
        }

        impl FindingCode {
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

finding_codes! {
    /// The text is not valid YAML or not UTF-8.
    YamlSyntax => "yaml_syntax",
    /// A field the schema does not know.
    UnknownField => "unknown_field",
    /// A required field is absent.
    MissingField => "missing_field",
    /// A value has the wrong type or shape.
    InvalidValue => "invalid_value",
    /// A key appears twice in one mapping.
    DuplicateKey => "duplicate_key",
    /// A tagged value or key, a merge key, or aliases that expand the file
    /// past its budget; plain anchors and aliases are accepted.
    YamlAlias => "yaml_alias",
    /// `apiVersion` is not `stackly/v1alpha1`.
    UnsupportedApiVersion => "unsupported_api_version",
    /// `kind` is not `Environment`.
    UnsupportedKind => "unsupported_kind",
    /// `metadata.name` or a service alias is not a valid name.
    InvalidName => "invalid_name",
    /// More than 32 services.
    TooManyServices => "too_many_services",
    /// More than 31 `dependsOn` entries in one service.
    TooManyDependencies => "too_many_dependencies",
    /// The provider is not in the catalog.
    UnknownProvider => "unknown_provider",
    /// The provider's package does not load.
    ProviderUnreadable => "provider_unreadable",
    /// `ownership` does not match `instanceRef`.
    OwnershipConflict => "ownership_conflict",
    /// A shared service writes a field only a dedicated one may write.
    InstanceRefConflict => "instance_ref_conflict",
    /// Two services borrow the same instance.
    DuplicateInstanceRef => "duplicate_instance_ref",
    /// The instance name, given or derived, is not a valid name.
    InstanceNameInvalid => "instance_name_invalid",
    /// Two services would create instances with the same name.
    InstanceNameDuplicate => "instance_name_duplicate",
    /// The package has no endpoint of that name.
    UnknownEndpoint => "unknown_endpoint",
    /// The endpoint is a Unix socket, which has no port.
    EndpointNotIp => "endpoint_not_ip",
    /// The host is not the endpoint's default host.
    EndpointHostUnsupported => "endpoint_host_unsupported",
    /// The transport is not the endpoint's transport.
    EndpointTransportMismatch => "endpoint_transport_mismatch",
    /// Two fixed endpoints use the same host and port.
    EndpointConflict => "endpoint_conflict",
    /// The package's settings schema has no such field.
    UnknownSetting => "unknown_setting",
    /// A settings value the schema refuses.
    SettingsValue => "settings_value",
    /// A value v1alpha1 does not support.
    UnsupportedValue => "unsupported_value",
    /// The package has no secret field of that name.
    UnknownSecret => "unknown_secret",
    /// A secret reference that is not a `local:` reference.
    SecretLiteral => "secret_literal",
    /// A secret reference in a namespace Stackly reserves.
    SecretReferenceReserved => "secret_reference_reserved",
    /// A secret field written in `settings`.
    SecretInSettings => "secret_in_settings",
    /// `dependsOn` names a service the manifest does not declare.
    UnknownDependency => "unknown_dependency",
    /// A service depends on itself.
    SelfDependency => "self_dependency",
    /// A service names one dependency twice.
    DuplicateDependency => "duplicate_dependency",
    /// The dependencies form a cycle.
    DependencyCycle => "dependency_cycle",
    /// A local override names a service the manifest does not declare.
    OverrideUnknownService => "override_unknown_service",
    /// A local override names a service that borrows its instance.
    OverrideNotAllowed => "override_not_allowed",
    /// Two fixed endpoints use the same host and port after the local
    /// overrides.
    OverrideEndpointConflict => "override_endpoint_conflict",
    /// `storage.dataLocation` is not an absolute path without `.` or `..`.
    StorageLocationInvalid => "storage_location_invalid",
}

impl FindingCode {
    /// The codes only the local override file has (spec 0045, *Aturan
    /// override*); a manifest never gets them.
    pub const LOCAL_ONLY: &'static [Self] = &[
        Self::OverrideUnknownService,
        Self::OverrideNotAllowed,
        Self::OverrideEndpointConflict,
        Self::StorageLocationInvalid,
    ];

    /// Returns the fixed message of the code; [`Finding`] adds a field
    /// name or a path of aliases where the code needs one.
    #[must_use]
    pub const fn template(self) -> &'static str {
        match self {
            Self::YamlSyntax => "the file is not valid YAML",
            Self::UnknownField => "unknown field",
            Self::MissingField => "missing field",
            Self::InvalidValue => "invalid value",
            Self::DuplicateKey => "a key appears twice in one mapping",
            Self::YamlAlias => {
                "YAML tags and merge keys are not allowed, and aliases can't expand the file past its size limit"
            }
            Self::UnsupportedApiVersion => "apiVersion must be stackly/v1alpha1",
            Self::UnsupportedKind => "kind must be Environment",
            Self::InvalidName => {
                "names use lowercase letters, digits, and single hyphens, 1 to 63 characters"
            }
            Self::TooManyServices => "a manifest declares 1 to 32 services",
            Self::TooManyDependencies => "a service has at most 31 dependencies",
            Self::UnknownProvider => "the provider is not in the catalog",
            Self::ProviderUnreadable => "the provider's package can't be loaded",
            Self::OwnershipConflict => "ownership is shared exactly when instanceRef is written",
            Self::InstanceRefConflict => "a service with instanceRef can't set",
            Self::DuplicateInstanceRef => "another service already borrows this instance",
            Self::InstanceNameInvalid => {
                "the instance name is not valid; write instanceName with at most 63 characters"
            }
            Self::InstanceNameDuplicate => "another service creates an instance with this name",
            Self::UnknownEndpoint => "the provider has no endpoint of this name",
            Self::EndpointNotIp => "this endpoint is a Unix socket and has no port",
            Self::EndpointHostUnsupported => "host must be the endpoint's default host",
            Self::EndpointTransportMismatch => "transport must be the endpoint's transport",
            Self::EndpointConflict => "another fixed endpoint uses the same host and port",
            Self::UnknownSetting => "the provider's settings have no such field",
            Self::SettingsValue => "the provider's settings refuse this value",
            Self::UnsupportedValue => "stackly/v1alpha1 does not support this value",
            Self::UnknownSecret => "the provider has no secret field of this name",
            Self::SecretLiteral => {
                "write a secret reference such as local:project/name, never the secret itself"
            }
            Self::SecretReferenceReserved => {
                "references under local:instance/ and local:stackly/ are reserved"
            }
            Self::SecretInSettings => "secret fields go in secretRefs, not settings",
            Self::UnknownDependency => "dependsOn names a service this manifest does not declare",
            Self::SelfDependency => "a service can't depend on itself",
            Self::DuplicateDependency => "this dependency is listed twice",
            Self::DependencyCycle => "dependency cycle",
            Self::OverrideUnknownService => {
                "the manifest declares no service of this name; the override is ignored"
            }
            Self::OverrideNotAllowed => {
                "a service with instanceRef borrows its instance and can't be overridden"
            }
            Self::OverrideEndpointConflict => {
                "after the local overrides, another fixed endpoint uses the same host and port"
            }
            Self::StorageLocationInvalid => {
                "dataLocation must be an absolute path of at most 1024 bytes without . or .."
            }
        }
    }
}

impl fmt::Display for FindingCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_code())
    }
}

/// Where a finding is: a position for syntax, a field path otherwise.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FindingLocation {
    /// The file as a whole.
    File,
    /// A line and column, both from 1.
    Position {
        /// The line.
        line: usize,
        /// The column.
        column: usize,
    },
    /// A field path such as `services.postgres.endpoints.client.port`.
    Field(String),
}

impl fmt::Display for FindingLocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::File => f.write_str("."),
            Self::Position { line, column } => write!(f, "{line}:{column}"),
            Self::Field(path) => f.write_str(path),
        }
    }
}

/// One problem in a manifest.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Finding {
    severity: Severity,
    code: FindingCode,
    location: FindingLocation,
    service: Option<MemberAlias>,
    message: String,
}

impl Finding {
    /// Creates an error with the code's template as its message.
    #[must_use]
    pub fn error(code: FindingCode, location: FindingLocation) -> Self {
        Self {
            severity: Severity::Error,
            code,
            location,
            service: None,
            message: code.template().to_owned(),
        }
    }

    /// Creates a warning with the code's template as its message.
    #[must_use]
    pub fn warning(code: FindingCode, location: FindingLocation) -> Self {
        Self {
            severity: Severity::Warning,
            ..Self::error(code, location)
        }
    }

    /// Replaces the message with another fixed one, for a code whose
    /// template names the manifest where another file is checked.
    #[must_use]
    pub(crate) fn with_message(mut self, message: &'static str) -> Self {
        self.message = message.to_owned();
        self
    }

    /// Appends a detail Stackly chose (a field name of the schema, or a
    /// path of valid aliases) to the template.
    #[must_use]
    pub fn with_detail(mut self, detail: impl fmt::Display) -> Self {
        self.message = format!("{}: {detail}", self.code.template());
        self
    }

    /// Names the service the finding is about.
    #[must_use]
    pub fn for_service(mut self, service: Option<&MemberAlias>) -> Self {
        self.service = service.cloned();
        self
    }

    /// Returns the severity.
    #[must_use]
    pub const fn severity(&self) -> Severity {
        self.severity
    }

    /// Returns the code.
    #[must_use]
    pub const fn code(&self) -> FindingCode {
        self.code
    }

    /// Returns where it is.
    #[must_use]
    pub const fn location(&self) -> &FindingLocation {
        &self.location
    }

    /// Returns the service it is about, if any.
    #[must_use]
    pub const fn service(&self) -> Option<&MemberAlias> {
        self.service.as_ref()
    }

    /// Returns the message: the template of the code, plus at most a name
    /// Stackly knows.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

/// Every finding of one check, at most [`MAX_FINDINGS`] kept.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Findings {
    list: Vec<Finding>,
    omitted: usize,
}

impl Findings {
    /// Adds a finding, or counts it past the limit.
    pub fn push(&mut self, finding: Finding) {
        if self.list.len() < MAX_FINDINGS {
            self.list.push(finding);
        } else {
            self.omitted += 1;
        }
    }

    /// Returns the kept findings, in the order they were found.
    #[must_use]
    pub fn list(&self) -> &[Finding] {
        &self.list
    }

    /// Returns how many findings there were, kept or not.
    #[must_use]
    pub const fn count(&self) -> usize {
        self.list.len() + self.omitted
    }

    /// Returns how many findings were dropped past the limit.
    #[must_use]
    pub const fn omitted(&self) -> usize {
        self.omitted
    }

    /// Returns whether any finding is an error. A dropped finding counts:
    /// every v1alpha1 code is an error.
    #[must_use]
    pub fn has_errors(&self) -> bool {
        self.omitted > 0 || self.list.iter().any(|f| f.severity == Severity::Error)
    }

    /// Returns how many kept findings have `severity`.
    #[must_use]
    pub fn count_of(&self, severity: Severity) -> usize {
        self.list.iter().filter(|f| f.severity == severity).count()
    }
}
