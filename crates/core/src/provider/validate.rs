//! `ValidatedPackage::from_files`: the two stage decode and every semantic
//! rule, without IO.

use std::collections::{BTreeMap, BTreeSet};
use std::net::IpAddr;
use std::path::PathBuf;

use stackly_domain::{
    Arch, Capability, CapabilitySet, EndpointName, InstanceId, InstanceName, LineCompatibility, Os,
    Port, ProviderName, ProviderRevision, ProviderRevisionKey, RevisionLabel, RuntimeLine,
    SoftwareName, Target,
};
use stackly_platform::{DirectoryLayout, RootSet};

use super::connection;
use super::digest::manifest_sha256;
use super::interpolate::{
    EndpointTransport, InstanceDir, Interp, PathExpr, PathPosition, Token, TypeEnv, ValuePosition,
    check_executable, check_literal, check_path, check_secret_file_name, check_socket_name,
    check_value,
};
use super::issue::{FileIssues, Issue, IssueCode, Issues, Location};
use super::manifest::{
    Category, CommandSpec, ConfigFileSpec, DataMarker, EndpointPurpose, EndpointSpec, HealthSpec,
    InitStepSpec, ManagedField, Manifest, ProbeDef, ReadinessSpec, ReloadActionSpec, ReloadSpec,
    StdinSpec, StopSignal, StopSpec,
};
use super::names::{
    COMPONENT_RULE, NAME_RULE, SettingName, env_key_problem, is_component, is_name,
    is_template_name,
};
use super::package::{
    MANIFEST_FILE, NameCheck, PackageError, PackageFiles, SETTINGS_SCHEMA_FILE, VERSIONS_FILE,
    check_files,
};
use super::plan::{PlanContext, ResolvedEndpoint, RuntimeContext};
use super::raw::{
    API_VERSION, KIND, RawCommand, RawManifest, RawProbe, RawSettingsSchema, RawStdin, RawStep,
    RawVersions, check_yaml_value, decode_json, decode_json_text, decode_yaml, parse_json,
    parse_yaml,
};
use super::settings::{ApplyClass, FieldKind, ScalarKind, SettingsSchema, parse_schema};
use super::template::{self, Checked, TemplateFormat};
use super::timeouts::{
    ATTEMPT_TIMEOUT_MS, Bound, DEFAULT_SUCCESS_EXIT_CODES, FAILURE_THRESHOLD,
    HEALTH_INTERVAL_SECONDS, INIT_SECONDS, READINESS_INTERVAL_MS, READINESS_SECONDS,
    RECOVERY_SECONDS, RELOAD_SECONDS, STOP_SECONDS, SUCCESS_EXIT_CODES_MAX, Timeouts,
};
use super::version_check::{self, VersionCheck};
use super::versions::{VersionCatalog, VersionsContext, check_versions};

/// The `file` of issues about the settings schema.
const SETTINGS_ISSUE_FILE: &str = "settings.schema.json";
/// Most endpoints.
const ENDPOINTS_MAX: usize = 16;
/// Most config files.
const CONFIG_FILES_MAX: usize = 16;
/// Most initialize steps.
const INIT_STEPS_MAX: usize = 16;
/// Most `args` entries.
const ARGS_MAX: usize = 128;
/// Most `env` keys.
const ENV_MAX: usize = 32;
/// Most `logs.files` entries.
const LOG_FILES_MAX: usize = 8;
/// Most executable aliases.
const ALIASES_MAX: usize = 16;
/// Most names in `lifecycle.reload.render`.
const RELOAD_RENDER_MAX: usize = 16;
/// Longest `displayName`, in characters.
const DISPLAY_NAME_MAX: usize = 64;
/// Longest `description`, in characters.
const DESCRIPTION_MAX: usize = 512;
/// Longest HTTP probe path, in bytes.
const HTTP_PATH_MAX: usize = 256;
/// Longest literal stdin text of a `runExecutable` step, in bytes (spec 0019).
const STDIN_TEXT_MAX: usize = 16 * 1024;

/// A provider package that passed every rule: the manifest, settings
/// schema, and templates in their checked form, its digest, and the files
/// it came from.
#[derive(Debug, Clone)]
pub struct ValidatedPackage {
    pub(crate) files: PackageFiles,
    pub(crate) key: ProviderRevisionKey,
    pub(crate) revision: ProviderRevision,
    pub(crate) manifest: Manifest,
    pub(crate) settings: SettingsSchema,
    pub(crate) templates: BTreeMap<String, Checked>,
    pub(crate) versions: VersionCatalog,
}

impl ValidatedPackage {
    /// Validates a package in memory, without IO.
    ///
    /// `apiVersion` and `kind` are decoded first; the rest of
    /// `provider.yaml` is decoded only when they are `stackly/v1alpha1` and
    /// `Provider`. Each file's structural decode reports at most one issue,
    /// because serde stops at the first error; the semantic rules then
    /// collect every problem up to [`MAX_ISSUES`](super::MAX_ISSUES).
    ///
    /// # Errors
    ///
    /// Returns a [`PackageError`] with every issue found.
    pub fn from_files(files: PackageFiles, name_check: NameCheck) -> Result<Self, PackageError> {
        let mut issues = Issues::new();
        let result = validate(&files, name_check, &mut issues);
        match result {
            Some((manifest, settings, templates, Some(versions))) if issues.is_empty() => {
                let mut hashed: Vec<(&str, &[u8])> = Vec::new();
                for path in [MANIFEST_FILE, SETTINGS_SCHEMA_FILE] {
                    if let Some(bytes) = files.get(path) {
                        hashed.push((path, bytes));
                    }
                }
                let template_paths: BTreeSet<String> = templates
                    .keys()
                    .map(|name| format!("templates/{name}"))
                    .collect();
                for path in &template_paths {
                    if let Some(bytes) = files.get(path) {
                        hashed.push((path.as_str(), bytes));
                    }
                }
                let key = ProviderRevisionKey::new(
                    manifest.id.clone(),
                    manifest.revision.clone(),
                    manifest_sha256(hashed),
                );
                let revision = ProviderRevision::new(
                    key.clone(),
                    manifest.software.clone(),
                    manifest.capabilities.clone(),
                    manifest.line_compatibility.clone(),
                );
                Ok(Self {
                    files,
                    key,
                    revision,
                    manifest,
                    settings,
                    templates,
                    versions,
                })
            }
            _ => Err(PackageError {
                package: files.folder().to_owned(),
                issues: issues.into_vec(),
            }),
        }
    }

    /// Returns the key that names this revision: ID, revision, and
    /// `manifest_sha256`.
    #[must_use]
    pub const fn key(&self) -> &ProviderRevisionKey {
        &self.key
    }

    /// Returns the software it provides.
    #[must_use]
    pub const fn software(&self) -> &SoftwareName {
        &self.manifest.software
    }

    /// Returns the checked manifest.
    #[must_use]
    pub const fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// Returns the endpoints the manifest declares, in name order.
    pub fn endpoints(&self) -> impl Iterator<Item = &EndpointSpec> {
        self.manifest.endpoints.values()
    }

    /// Returns the typed settings model.
    #[must_use]
    pub const fn settings_schema(&self) -> &SettingsSchema {
        &self.settings
    }

    /// Returns the domain provider revision, with the capabilities and line
    /// compatibility of the manifest.
    #[must_use]
    pub const fn provider_revision(&self) -> &ProviderRevision {
        &self.revision
    }

    /// Returns the files the package was validated from, for pinning a
    /// copy that reads back with the same digest.
    #[must_use]
    pub const fn files(&self) -> &PackageFiles {
        &self.files
    }

    /// Returns the typed `versions.json`: every artifact in file order, for
    /// the catalog (spec 0017) and the installer (spec 0018).
    #[must_use]
    pub const fn versions(&self) -> &VersionCatalog {
        &self.versions
    }

    /// Returns `runtime.versionCheck`, which the installer runs once from
    /// staging (spec 0018, *Validasi binary* step 8).
    #[must_use]
    pub const fn version_check(&self) -> Option<&VersionCheck> {
        self.manifest.version_check()
    }
}

type Validated = (
    Manifest,
    SettingsSchema,
    BTreeMap<String, Checked>,
    Option<VersionCatalog>,
);

fn validate(files: &PackageFiles, name_check: NameCheck, issues: &mut Issues) -> Option<Validated> {
    let texts = check_files(files, issues);
    let raw_manifest = texts
        .get(MANIFEST_FILE)
        .and_then(|text| decode_manifest(text, issues));
    let raw_settings = texts.get(SETTINGS_SCHEMA_FILE).and_then(|text| {
        parse_json(SETTINGS_ISSUE_FILE, text)
            .map_err(|issue| issues.push(issue))
            .ok()?;
        decode_json_text::<RawSettingsSchema>(
            SETTINGS_ISSUE_FILE,
            text,
            IssueCode::SettingsSchemaKeyword,
        )
        .map_err(|issue| issues.push(issue))
        .ok()
    });
    let settings = raw_settings
        .and_then(|raw| parse_schema(raw, &mut FileIssues::new(SETTINGS_ISSUE_FILE, issues)));
    let raw_versions = texts.get(VERSIONS_FILE).and_then(|text| {
        let value = parse_json(VERSIONS_FILE, text)
            .map_err(|issue| issues.push(issue))
            .ok()?;
        decode_json::<RawVersions>(VERSIONS_FILE, value, IssueCode::UnknownField)
            .map_err(|issue| issues.push(issue))
            .ok()
    });
    let mut parsed = BTreeMap::new();
    for (path, text) in &texts {
        let Some(name) = path.strip_prefix("templates/") else {
            continue;
        };
        match template::parse(text) {
            Ok(template) => {
                parsed.insert(name.to_owned(), template);
            }
            Err((at, message)) => issues.push(Issue::new(
                *path,
                Location::Position {
                    line: at.line,
                    column: at.column,
                },
                IssueCode::TemplateSyntax,
                message,
            )),
        }
    }
    let template_names: BTreeSet<String> = files
        .iter()
        .filter_map(|(path, _)| path.strip_prefix("templates/"))
        .filter(|name| is_template_name(name))
        .map(str::to_owned)
        .collect();
    let raw = raw_manifest?;
    let mut checker = Checker {
        issues,
        folder: files.folder(),
        name_check,
        settings: settings.as_ref(),
        template_names: &template_names,
        refs: Vec::new(),
    };
    let manifest = checker.manifest(&raw);
    let Checker { refs, .. } = checker;
    let manifest = manifest?;
    let templates = check_templates(
        &manifest,
        settings.as_ref(),
        &parsed,
        &template_names,
        issues,
    );
    check_setting_refs(&manifest, settings.as_ref(), &refs, &templates, issues);
    let mut versions = None;
    if let Some(raw) = &raw_versions {
        let mut executables = vec![(
            manifest.executable.join("/"),
            "runtime.executable".to_owned(),
        )];
        for (alias, path) in &manifest.aliases {
            executables.push((
                path.join("/"),
                format!("permissions.additionalExecutables.{alias}"),
            ));
        }
        let context = VersionsContext {
            software: Some(&manifest.software),
            testing: manifest.id.namespace() == "testing",
            platforms: &manifest.platforms,
            executables,
        };
        versions = check_versions(raw, &context, &mut FileIssues::new(VERSIONS_FILE, issues));
    }
    let settings = settings?;
    let templates = templates?;
    if issues.is_empty() {
        sample_renders(&manifest, &settings, &templates, issues);
    }
    if issues.is_empty() {
        connection::check_sentinels(&manifest, &settings, issues);
    }
    Some((manifest, settings, templates, versions))
}

fn decode_manifest(text: &str, issues: &mut Issues) -> Option<RawManifest> {
    let file = MANIFEST_FILE;
    let value = parse_yaml(file, text)
        .map_err(|issue| issues.push(issue))
        .ok()?;
    if let Err(issue) = check_yaml_value(file, &value) {
        issues.push(issue);
        return None;
    }
    let mut yaml = FileIssues::new(file, issues);
    if !value.is_mapping() {
        yaml.at(
            ".",
            IssueCode::InvalidValue,
            "provider.yaml must be a mapping",
        );
        return None;
    }
    for (field, expected, code) in [
        ("apiVersion", API_VERSION, IssueCode::UnsupportedApiVersion),
        ("kind", KIND, IssueCode::UnknownKind),
    ] {
        match value.get(field) {
            None => {
                yaml.at(
                    field,
                    IssueCode::MissingField,
                    format!("missing field `{field}`"),
                );
                return None;
            }
            Some(found) if found.as_str() == Some(expected) => {}
            Some(_) => {
                yaml.at(field, code, format!("{field} must be {expected}"));
                return None;
            }
        }
    }
    decode_yaml(file, value, IssueCode::UnknownField)
        .map_err(|issue| issues.push(issue))
        .ok()
}

/// Where a setting is referenced from, for the `apply: reload` rule.
#[derive(Debug, Clone)]
enum RefSite {
    Manifest(String),
    Template(String),
}

struct Checker<'a> {
    issues: &'a mut Issues,
    folder: &'a str,
    name_check: NameCheck,
    settings: Option<&'a SettingsSchema>,
    template_names: &'a BTreeSet<String>,
    refs: Vec<(SettingName, RefSite)>,
}

/// The parts of a command, from a command object or a flattened step.
struct CommandParts<'a> {
    executable: &'a str,
    args: &'a [String],
    cwd: Option<&'a str>,
    env: Option<&'a BTreeMap<String, String>>,
}

impl<'a> CommandParts<'a> {
    fn of(raw: &'a RawCommand) -> Self {
        Self {
            executable: &raw.executable,
            args: &raw.args,
            cwd: raw.cwd.as_deref(),
            env: Some(&raw.env),
        }
    }
}

impl Checker<'_> {
    fn yaml(&mut self) -> FileIssues<'_> {
        FileIssues::new(MANIFEST_FILE, self.issues)
    }

    fn at(&mut self, path: impl Into<String>, code: IssueCode, message: impl Into<String>) {
        self.yaml().at(path, code, message);
    }

    fn count(&self) -> usize {
        self.issues.count()
    }

    fn manifest(&mut self, raw: &RawManifest) -> Option<Manifest> {
        let start = self.count();
        let meta = &raw.metadata;
        let id = meta.id.parse::<ProviderName>().map_err(|_| {
            self.at(
                "metadata.id",
                IssueCode::InvalidName,
                "id must be <namespace>/<name>, each part a lowercase name (VAL-002)",
            );
        });
        if let (Ok(id), NameCheck::FolderName) = (&id, self.name_check)
            && id.name() != self.folder
        {
            let message = format!(
                "the name part of metadata.id must equal the package folder name {}",
                self.folder
            );
            self.at("metadata.id", IssueCode::DirectoryNameMismatch, message);
        }
        let software = meta.software.parse::<SoftwareName>().map_err(|_| {
            self.at(
                "metadata.software",
                IssueCode::InvalidName,
                "software must be a lowercase name",
            );
        });
        let revision = meta.revision.parse::<RevisionLabel>().map_err(|_| {
            self.at("metadata.revision", IssueCode::InvalidName, "revision must be 1 to 64 letters, digits, and . + _ -, starting with a letter or digit, without .. (VAL-002)");
        });
        if !text_ok(&meta.display_name, 1, DISPLAY_NAME_MAX) {
            self.at(
                "metadata.displayName",
                IssueCode::InvalidValue,
                "displayName must be 1 to 64 characters without control characters",
            );
        }
        if meta
            .description
            .as_deref()
            .is_some_and(|text| !text_ok(text, 0, DESCRIPTION_MAX))
        {
            self.at(
                "metadata.description",
                IssueCode::InvalidValue,
                "description must be at most 512 characters without control characters",
            );
        }
        let category = Category::from_code(&meta.category);
        match (category, &id) {
            (None, _) => self.at(
                "metadata.category",
                IssueCode::InvalidValue,
                "category must be database, messaging, cache, proxy, analytics, or testing",
            ),
            (Some(category), Ok(id))
                if (category == Category::Testing) != (id.namespace() == "testing") =>
            {
                self.at(
                    "metadata.category",
                    IssueCode::InvalidValue,
                    "category testing is used exactly when the namespace is testing",
                );
            }
            _ => {}
        }
        let platforms = self.platforms(raw);
        let line_compatibility = self.line_compatibility(raw);
        let data_marker = self.data_marker(raw);
        let executable = self.relative_path(&raw.runtime.executable, "runtime.executable");
        if raw.runtime.execution_mode != "foreground" {
            self.at(
                "runtime.executionMode",
                IssueCode::InvalidValue,
                "executionMode must be foreground",
            );
        }
        let managed_fs = self.managed_fs(raw);
        let aliases = self.aliases(raw);
        let timeouts = self.timeouts(raw);
        let endpoints = self.endpoints(raw, &managed_fs);
        let transports: BTreeMap<EndpointName, EndpointTransport> = endpoints
            .iter()
            .map(|(name, spec)| (name.clone(), spec.transport))
            .collect();
        let alias_names: BTreeSet<String> = aliases.keys().cloned().collect();
        let env = TypeEnv {
            endpoints: &transports,
            aliases: &alias_names,
            settings: self.settings,
            managed_fs: &managed_fs,
        };
        let endpoints = self.socket_names(raw, endpoints, &env);
        let managed_fields = self.managed_fields(&env);
        let config_files = self.config_files(raw, &env);
        let lifecycle = &raw.lifecycle;
        if lifecycle.initialize.len() > INIT_STEPS_MAX {
            self.at(
                "lifecycle.initialize",
                IssueCode::InvalidValue,
                format!("at most {INIT_STEPS_MAX} initialize steps"),
            );
        }
        let initialize: Vec<InitStepSpec> = lifecycle
            .initialize
            .iter()
            .enumerate()
            .filter_map(|(index, step)| {
                self.step(
                    step,
                    &format!("lifecycle.initialize[{index}]"),
                    &env,
                    &config_files,
                    timeouts.init,
                )
            })
            .collect();
        let start_command = self.command(
            &CommandParts::of(&lifecycle.start),
            "lifecycle.start",
            &env,
            true,
        );
        let stop = self.stop(raw, &env);
        let reload = self.reload(raw, &env, &config_files);
        let readiness = self.readiness(raw, &env, &timeouts, reload.as_ref());
        let health = self.health(raw, &env, readiness.as_ref());
        let log_files = self.log_files(raw, &env);
        let connections = connection::check(raw.connection.as_deref(), &env, &mut self.yaml());
        let version_check =
            version_check::check(raw.runtime.version_check.as_ref(), &mut self.yaml());
        let (capabilities, cold_backup_paths) =
            self.capabilities(raw, &endpoints, &managed_fs, reload.is_some());
        if self.count() != start {
            return None;
        }
        Some(Manifest {
            id: id.ok()?,
            software: software.ok()?,
            display_name: meta.display_name.clone(),
            revision: revision.ok()?,
            category: category?,
            description: meta.description.clone(),
            platforms,
            line_compatibility,
            data_marker,
            executable: executable?,
            capabilities,
            endpoints,
            config_files,
            initialize,
            start: start_command?,
            stop: stop?,
            reload,
            readiness: readiness?,
            health: health?,
            timeouts,
            log_files,
            cold_backup_paths,
            managed_fs,
            aliases,
            managed_fields,
            connections,
            version_check,
        })
    }

    fn platforms(&mut self, raw: &RawManifest) -> Vec<Target> {
        let list = &raw.compatibility.platforms;
        if list.is_empty() {
            self.at(
                "compatibility.platforms",
                IssueCode::InvalidValue,
                "list at least one platform",
            );
        }
        let mut platforms = Vec::new();
        for (index, platform) in list.iter().enumerate() {
            let path = format!("compatibility.platforms[{index}]");
            let os = platform.os.parse::<Os>();
            let arch = platform.arch.parse::<Arch>();
            if os.is_err() {
                self.at(
                    format!("{path}.os"),
                    IssueCode::InvalidValue,
                    "os must be a domain code such as macos",
                );
            }
            if arch.is_err() {
                self.at(
                    format!("{path}.arch"),
                    IssueCode::InvalidValue,
                    "arch must be a domain code such as aarch64",
                );
            }
            if let (Ok(os), Ok(arch)) = (os, arch) {
                let target = Target::new(os, arch);
                if platforms.contains(&target) {
                    self.at(path, IssueCode::InvalidValue, "each platform appears once");
                } else {
                    platforms.push(target);
                }
            }
        }
        platforms
    }

    fn line_compatibility(&mut self, raw: &RawManifest) -> LineCompatibility {
        let mut map = BTreeMap::new();
        for (data, runtimes) in &raw.compatibility.line_compatibility {
            let path = format!("compatibility.lineCompatibility.{data}");
            let Ok(data_line) = data.parse::<RuntimeLine>() else {
                self.at(
                    path,
                    IssueCode::InvalidValue,
                    "keys must be runtime lines such as 17 or 8.4",
                );
                continue;
            };
            let mut set = BTreeSet::new();
            for (index, runtime) in runtimes.iter().enumerate() {
                match runtime.parse::<RuntimeLine>() {
                    Ok(line) => {
                        set.insert(line);
                    }
                    Err(_) => self.at(
                        format!("{path}[{index}]"),
                        IssueCode::InvalidValue,
                        "values must be runtime lines such as 17 or 8.4",
                    ),
                }
            }
            map.insert(data_line, set);
        }
        LineCompatibility::from_map(map)
    }

    fn data_marker(&mut self, raw: &RawManifest) -> Option<DataMarker> {
        let marker = raw.compatibility.data_marker.as_ref()?;
        let path = "compatibility.dataMarker";
        let mut ok = true;
        if !is_component(&marker.file) {
            self.at(
                format!("{path}.file"),
                IssueCode::InvalidName,
                COMPONENT_RULE,
            );
            ok = false;
        }
        let pattern = match super::settings::Pattern::compile(&marker.pattern) {
            Ok(pattern) if pattern.regex().captures_len() == 2 => Some(pattern),
            Ok(_) => {
                self.at(
                    format!("{path}.pattern"),
                    IssueCode::InvalidValue,
                    "pattern needs exactly one capture group that yields the runtime line",
                );
                None
            }
            Err(message) => {
                self.at(format!("{path}.pattern"), IssueCode::InvalidValue, message);
                None
            }
        };
        let pattern = pattern?;
        ok.then(|| DataMarker {
            file: marker.file.clone(),
            pattern,
        })
    }

    /// Checks a relative path inside the runtime: literal components only.
    fn relative_path(&mut self, text: &str, path: &str) -> Option<Vec<String>> {
        if text.starts_with('/') || text.split('/').any(|part| part == "..") {
            self.at(
                path,
                IssueCode::PathEscapesScope,
                "the path must stay inside the runtime: relative, without ..",
            );
            return None;
        }
        if !text.split('/').all(is_component) {
            self.at(path, IssueCode::InvalidName, COMPONENT_RULE);
            return None;
        }
        Some(text.split('/').map(str::to_owned).collect())
    }

    fn managed_fs(&mut self, raw: &RawManifest) -> BTreeSet<InstanceDir> {
        let permissions = &raw.permissions;
        if !permissions.execute_runtime_binary {
            self.at(
                "permissions.executeRuntimeBinary",
                IssueCode::InvalidValue,
                "executeRuntimeBinary must be true in stackly/v1alpha1",
            );
        }
        let mut set = BTreeSet::new();
        for (index, code) in permissions.managed_filesystem.iter().enumerate() {
            let path = format!("permissions.managedFilesystem[{index}]");
            match InstanceDir::from_code(code) {
                Some(dir) if set.insert(dir) => {}
                Some(_) => self.at(path, IssueCode::InvalidValue, "each folder appears once"),
                None => self.at(
                    path,
                    IssueCode::InvalidValue,
                    "folders are data, config, logs, temp, or run",
                ),
            }
        }
        set
    }

    fn aliases(&mut self, raw: &RawManifest) -> BTreeMap<String, Vec<String>> {
        let list = &raw.permissions.additional_executables;
        if list.len() > ALIASES_MAX {
            self.at(
                "permissions.additionalExecutables",
                IssueCode::InvalidValue,
                format!("at most {ALIASES_MAX} aliases"),
            );
        }
        let mut aliases = BTreeMap::new();
        for (alias, text) in list {
            let path = format!("permissions.additionalExecutables.{alias}");
            if !is_name(alias) {
                self.at(
                    &path,
                    IssueCode::InvalidName,
                    format!("aliases {NAME_RULE}"),
                );
                continue;
            }
            if let Some(components) = self.relative_path(text, &path) {
                aliases.insert(alias.clone(), components);
            }
        }
        aliases
    }

    fn bounded(&mut self, value: Option<u64>, bound: Bound, path: &str) -> u64 {
        let value = value.unwrap_or(bound.default);
        if !bound.range().contains(&value) {
            self.at(
                path,
                IssueCode::TimeoutOutOfRange,
                format!("must be {} to {}", bound.min, bound.max),
            );
        }
        value
    }

    fn timeouts(&mut self, raw: &RawManifest) -> Timeouts {
        let t = &raw.timeouts;
        let init = self.bounded(t.init_seconds, INIT_SECONDS, "timeouts.initSeconds");
        let readiness = self.bounded(
            t.readiness_seconds,
            READINESS_SECONDS,
            "timeouts.readinessSeconds",
        );
        let recovery_bound = Bound {
            default: readiness,
            ..RECOVERY_SECONDS
        };
        let recovery = self.bounded(
            t.recovery_seconds,
            recovery_bound,
            "timeouts.recoverySeconds",
        );
        let stop = self.bounded(t.stop_seconds, STOP_SECONDS, "timeouts.stopSeconds");
        let reload = self.bounded(t.reload_seconds, RELOAD_SECONDS, "timeouts.reloadSeconds");
        if t.reload_seconds.is_some() && !raw.capabilities.reload {
            self.at(
                "timeouts.reloadSeconds",
                IssueCode::ReloadWithoutCapability,
                "reloadSeconds needs the reload capability",
            );
        }
        Timeouts {
            init,
            readiness,
            recovery,
            stop,
            reload,
        }
    }

    fn endpoints(
        &mut self,
        raw: &RawManifest,
        managed_fs: &BTreeSet<InstanceDir>,
    ) -> BTreeMap<EndpointName, EndpointSpec> {
        let list = &raw.resources.endpoints;
        if list.is_empty() || list.len() > ENDPOINTS_MAX {
            self.at(
                "resources.endpoints",
                IssueCode::InvalidValue,
                format!("declare 1 to {ENDPOINTS_MAX} endpoints"),
            );
        }
        let mut endpoints = BTreeMap::new();
        for (name, endpoint) in list {
            let path = format!("resources.endpoints.{name}");
            let parsed = name.parse::<EndpointName>().ok().filter(|_| is_name(name));
            let Some(endpoint_name) = parsed else {
                self.at(&path, IssueCode::InvalidName, "endpoint names are 1 to 32 lowercase letters and digits, starting with a letter");
                continue;
            };
            let transport = match endpoint.transport.as_str() {
                "tcp" => Some(EndpointTransport::Tcp),
                "udp" => Some(EndpointTransport::Udp),
                "unix" => Some(EndpointTransport::Unix),
                _ => {
                    self.at(
                        format!("{path}.transport"),
                        IssueCode::InvalidValue,
                        "transport must be tcp, udp, or unix",
                    );
                    None
                }
            };
            let purpose = EndpointPurpose::from_code(&endpoint.purpose);
            if purpose.is_none() {
                self.at(
                    format!("{path}.purpose"),
                    IssueCode::InvalidValue,
                    "purpose must be client, monitoring, or admin",
                );
            }
            let (Some(transport), Some(purpose)) = (transport, purpose) else {
                continue;
            };
            let mut spec = EndpointSpec {
                name: endpoint_name.clone(),
                transport,
                purpose,
                default_host: None,
                default_port: None,
                socket_name: None,
            };
            if transport.is_ip() {
                if endpoint.socket_name.is_some() {
                    self.at(
                        format!("{path}.socketName"),
                        IssueCode::UnknownField,
                        "socketName belongs only to unix endpoints",
                    );
                }
                match endpoint.default_host.as_deref().map(str::parse::<IpAddr>) {
                    None => self.at(
                        format!("{path}.defaultHost"),
                        IssueCode::MissingField,
                        "tcp and udp endpoints need defaultHost",
                    ),
                    Some(Err(_)) => self.at(
                        format!("{path}.defaultHost"),
                        IssueCode::InvalidValue,
                        "defaultHost must be an IP address",
                    ),
                    Some(Ok(host)) if is_loopback_default(host) => spec.default_host = Some(host),
                    Some(Ok(_)) => self.at(
                        format!("{path}.defaultHost"),
                        IssueCode::EndpointNotLoopback,
                        "defaultHost must be 127.0.0.1 or ::1",
                    ),
                }
                match endpoint.default_port.map(|port| {
                    u16::try_from(port)
                        .ok()
                        .and_then(|port| Port::new(port).ok())
                }) {
                    None => self.at(
                        format!("{path}.defaultPort"),
                        IssueCode::MissingField,
                        "tcp and udp endpoints need defaultPort",
                    ),
                    Some(None) => self.at(
                        format!("{path}.defaultPort"),
                        IssueCode::InvalidValue,
                        "defaultPort must be 1 to 65535",
                    ),
                    Some(Some(port)) => spec.default_port = Some(port),
                }
            } else {
                for (field, set) in [
                    ("defaultHost", endpoint.default_host.is_some()),
                    ("defaultPort", endpoint.default_port.is_some()),
                ] {
                    if set {
                        self.at(
                            format!("{path}.{field}"),
                            IssueCode::UnknownField,
                            format!("{field} belongs only to tcp and udp endpoints"),
                        );
                    }
                }
                if endpoint.socket_name.is_none() {
                    self.at(
                        format!("{path}.socketName"),
                        IssueCode::MissingField,
                        "unix endpoints need socketName",
                    );
                }
                if !managed_fs.contains(&InstanceDir::Run) {
                    self.at(
                        &path,
                        IssueCode::PathNotPermitted,
                        "unix endpoints need run in permissions.managedFilesystem",
                    );
                }
            }
            endpoints.insert(endpoint_name, spec);
        }
        endpoints
    }

    fn socket_names(
        &mut self,
        raw: &RawManifest,
        mut endpoints: BTreeMap<EndpointName, EndpointSpec>,
        env: &TypeEnv<'_>,
    ) -> BTreeMap<EndpointName, EndpointSpec> {
        for (name, spec) in &mut endpoints {
            let Some(text) = raw
                .resources
                .endpoints
                .get(name.as_str())
                .and_then(|endpoint| endpoint.socket_name.as_deref())
            else {
                continue;
            };
            if spec.transport == EndpointTransport::Unix {
                let path = format!("resources.endpoints.{name}.socketName");
                spec.socket_name = check_socket_name(text, env, &mut self.yaml(), &path);
            }
        }
        endpoints
    }

    fn managed_fields(&mut self, env: &TypeEnv<'_>) -> Vec<ManagedField> {
        let Some(schema) = self.settings else {
            return Vec::new();
        };
        let mut fields = Vec::new();
        for field in schema.fields() {
            let Some(expression) = field.managed() else {
                continue;
            };
            let path = format!("properties.{}.x-stackly.managed", field.name());
            let integer = matches!(field.kind(), FieldKind::Scalar(ScalarKind::Integer(_)));
            let mut issues = FileIssues::new(SETTINGS_ISSUE_FILE, self.issues);
            let Some(interp) =
                check_value(expression, ValuePosition::Managed, env, &mut issues, &path)
            else {
                continue;
            };
            if integer
                && !matches!(
                    interp.pieces.as_slice(),
                    [super::interpolate::Piece::Token(Token::EndpointPort(_))]
                )
            {
                issues.at(
                    &path,
                    IssueCode::SettingsSchemaRule,
                    "a managed integer is exactly one ${endpoint.<name>.port} token",
                );
                continue;
            }
            fields.push(ManagedField {
                name: field.name().clone(),
                expression: interp,
                integer,
            });
        }
        fields
    }

    fn config_files(
        &mut self,
        raw: &RawManifest,
        env: &TypeEnv<'_>,
    ) -> BTreeMap<String, ConfigFileSpec> {
        if raw.config_files.len() > CONFIG_FILES_MAX {
            self.at(
                "configFiles",
                IssueCode::InvalidValue,
                format!("at most {CONFIG_FILES_MAX} config files"),
            );
        }
        let mut files = BTreeMap::new();
        for (name, file) in &raw.config_files {
            let path = format!("configFiles.{name}");
            if !is_name(name) {
                self.at(
                    &path,
                    IssueCode::InvalidName,
                    format!("config file names {NAME_RULE}"),
                );
                continue;
            }
            let template = match file.template.strip_prefix("templates/") {
                Some(template) if self.template_names.contains(template) => {
                    Some(template.to_owned())
                }
                _ => {
                    self.at(
                        format!("{path}.template"),
                        IssueCode::MissingFile,
                        "template must name a file templates/<name>.tmpl in the package",
                    );
                    None
                }
            };
            let format = TemplateFormat::from_code(&file.format);
            if format.is_none() {
                self.at(
                    format!("{path}.format"),
                    IssueCode::UnknownTemplateFormat,
                    "format must be yaml, json, natsConf, postgresqlConf, or plain",
                );
            }
            let destination = check_path(
                &file.destination,
                PathPosition::ConfigDestination {
                    sensitive: file.sensitive,
                },
                env,
                &mut self.yaml(),
                &format!("{path}.destination"),
            );
            if let (Some(template), Some(format), Some(destination)) =
                (template, format, destination)
            {
                files.insert(
                    name.clone(),
                    ConfigFileSpec {
                        template,
                        format,
                        destination,
                        sensitive: file.sensitive,
                    },
                );
            }
        }
        files
    }

    fn record_refs(&mut self, interp: &Interp, path: &str) {
        for token in interp.tokens() {
            if let Token::Setting(name) = token {
                self.refs
                    .push((name.clone(), RefSite::Manifest(path.to_owned())));
            }
        }
    }

    /// Checks a command. `secrets` is true for the commands that resolve
    /// secrets (`lifecycle.start` and `runExecutable`), whose env values
    /// may each be one whole `${secret.<field>}` token.
    fn command(
        &mut self,
        parts: &CommandParts<'_>,
        path: &str,
        env: &TypeEnv<'_>,
        secrets: bool,
    ) -> Option<CommandSpec> {
        let start = self.count();
        let executable = check_executable(
            parts.executable,
            env,
            &mut self.yaml(),
            &format!("{path}.executable"),
        );
        if parts.args.len() > ARGS_MAX {
            self.at(
                format!("{path}.args"),
                IssueCode::InvalidValue,
                format!("at most {ARGS_MAX} arguments"),
            );
        }
        let mut args = Vec::new();
        for (index, arg) in parts.args.iter().enumerate() {
            let at = format!("{path}.args[{index}]");
            if let Some(interp) = check_value(arg, ValuePosition::Arg, env, &mut self.yaml(), &at) {
                self.record_refs(&interp, &at);
                args.push(interp);
            }
        }
        let cwd = check_path(
            parts.cwd.unwrap_or("${instance.data}"),
            PathPosition::Cwd,
            env,
            &mut self.yaml(),
            &format!("{path}.cwd"),
        );
        let mut env_list = Vec::new();
        if let Some(vars) = parts.env {
            if vars.len() > ENV_MAX {
                self.at(
                    format!("{path}.env"),
                    IssueCode::InvalidValue,
                    format!("at most {ENV_MAX} env keys"),
                );
            }
            for (key, value) in vars {
                let at = format!("{path}.env.{key}");
                if let Some(problem) = env_key_problem(key) {
                    self.at(&at, IssueCode::InvalidName, problem);
                    continue;
                }
                if let Some(interp) = check_value(
                    value,
                    ValuePosition::EnvValue { secrets },
                    env,
                    &mut self.yaml(),
                    &at,
                ) {
                    self.record_refs(&interp, &at);
                    env_list.push((key.clone(), interp));
                }
            }
        }
        if self.count() != start {
            return None;
        }
        Some(CommandSpec {
            executable: executable?,
            args,
            cwd: cwd?,
            env: env_list,
            at: path.to_owned(),
        })
    }

    fn success_codes(&mut self, codes: Option<&Vec<u8>>, path: &str) -> Vec<u8> {
        match codes {
            None => DEFAULT_SUCCESS_EXIT_CODES.to_vec(),
            Some(codes) if (1..=SUCCESS_EXIT_CODES_MAX).contains(&codes.len()) => codes.clone(),
            Some(_) => {
                self.at(
                    path,
                    IssueCode::InvalidValue,
                    format!("successExitCodes holds 1 to {SUCCESS_EXIT_CODES_MAX} codes"),
                );
                DEFAULT_SUCCESS_EXIT_CODES.to_vec()
            }
        }
    }

    /// Reports every field that does not belong to the chosen variant.
    fn reject_fields(&mut self, path: &str, owner: &str, fields: &[(&str, bool)]) {
        for (field, set) in fields {
            if *set {
                self.at(
                    format!("{path}.{field}"),
                    IssueCode::UnknownField,
                    format!("{field} does not belong to {owner}"),
                );
            }
        }
    }

    /// Checks the stdin of a `runExecutable` step: exactly one of a secret
    /// field or literal text (spec 0019). The text is written exactly, so
    /// it holds no `${` (it is never interpolated) and no control
    /// character other than a newline or a tab.
    fn stdin(&mut self, stdin: &RawStdin, path: &str) -> Option<StdinSpec> {
        match (&stdin.secret, &stdin.text) {
            (Some(name), None) => self
                .secret_field(name, &format!("{path}.secret"))
                .map(StdinSpec::Secret),
            (None, Some(text)) => {
                let at = format!("{path}.text");
                let start = self.count();
                if text.is_empty() || text.len() > STDIN_TEXT_MAX {
                    self.at(
                        &at,
                        IssueCode::InvalidValue,
                        format!("stdin text must be 1 to {STDIN_TEXT_MAX} bytes"),
                    );
                }
                if text.contains("${") {
                    self.at(
                        &at,
                        IssueCode::TokenNotAllowedHere,
                        "stdin text is literal and never interpolated, so it holds no ${",
                    );
                }
                if text
                    .chars()
                    .any(|c| c.is_control() && !matches!(c, '\n' | '\t'))
                {
                    self.at(
                        &at,
                        IssueCode::InvalidValue,
                        "stdin text holds no control character other than a newline or a tab",
                    );
                }
                (self.count() == start).then(|| StdinSpec::Text(text.clone()))
            }
            _ => {
                self.at(
                    path,
                    IssueCode::InvalidValue,
                    "stdin takes exactly one of secret and text",
                );
                None
            }
        }
    }

    fn secret_field(&mut self, name: &str, path: &str) -> Option<SettingName> {
        let schema = self.settings?;
        match schema.field(name) {
            Some(field) if field.is_secret() => Some(field.name().clone()),
            _ => {
                self.at(
                    path,
                    IssueCode::UnknownSecret,
                    format!("the settings schema has no secret field {name}"),
                );
                None
            }
        }
    }

    fn step(
        &mut self,
        step: &RawStep,
        path: &str,
        env: &TypeEnv<'_>,
        config_files: &BTreeMap<String, ConfigFileSpec>,
        init_seconds: u64,
    ) -> Option<InitStepSpec> {
        let fields = [
            ("file", step.file.is_some()),
            ("secret", step.secret.is_some()),
            ("destination", step.destination.is_some()),
            ("executable", step.executable.is_some()),
            ("args", step.args.is_some()),
            ("cwd", step.cwd.is_some()),
            ("env", step.env.is_some()),
            ("stdin", step.stdin.is_some()),
            ("timeoutSeconds", step.timeout_seconds.is_some()),
            ("successExitCodes", step.success_exit_codes.is_some()),
        ];
        let allowed: &[&str] = match step.primitive.as_str() {
            "createManagedDirectories" => &[],
            "renderConfig" => &["file"],
            "materializeSecret" => &["secret", "destination"],
            "runExecutable" => &[
                "executable",
                "args",
                "cwd",
                "env",
                "stdin",
                "timeoutSeconds",
                "successExitCodes",
            ],
            other => {
                let message = format!(
                    "unknown primitive {other}; use createManagedDirectories, renderConfig, materializeSecret, or runExecutable"
                );
                self.at(
                    format!("{path}.primitive"),
                    IssueCode::UnknownPrimitive,
                    message,
                );
                return None;
            }
        };
        let extra: Vec<(&str, bool)> = fields
            .iter()
            .copied()
            .filter(|(name, _)| !allowed.contains(name))
            .collect();
        let owner = step.primitive.clone();
        self.reject_fields(path, &owner, &extra);
        let missing = |checker: &mut Self, field: &str| {
            checker.at(
                format!("{path}.{field}"),
                IssueCode::MissingField,
                format!("{owner} needs {field}"),
            );
        };
        match step.primitive.as_str() {
            "createManagedDirectories" => Some(InitStepSpec::CreateManagedDirectories),
            "renderConfig" => {
                let Some(file) = &step.file else {
                    missing(self, "file");
                    return None;
                };
                if config_files.contains_key(file) {
                    Some(InitStepSpec::RenderConfig(file.clone()))
                } else {
                    self.at(
                        format!("{path}.file"),
                        IssueCode::UnknownConfigFile,
                        format!("configFiles has no {file}"),
                    );
                    None
                }
            }
            "materializeSecret" => {
                let secret = match &step.secret {
                    Some(name) => self.secret_field(name, &format!("{path}.secret")),
                    None => {
                        missing(self, "secret");
                        None
                    }
                };
                let file_name = match &step.destination {
                    Some(text) => check_secret_file_name(
                        text,
                        env,
                        &mut self.yaml(),
                        &format!("{path}.destination"),
                    ),
                    None => {
                        missing(self, "destination");
                        None
                    }
                };
                Some(InitStepSpec::MaterializeSecret {
                    secret: secret?,
                    file_name: file_name?,
                })
            }
            _ => {
                let Some(executable) = &step.executable else {
                    missing(self, "executable");
                    return None;
                };
                let no_args = Vec::new();
                let parts = CommandParts {
                    executable,
                    args: step.args.as_ref().unwrap_or(&no_args),
                    cwd: step.cwd.as_deref(),
                    env: step.env.as_ref(),
                };
                let command = self.command(&parts, path, env, true);
                let stdin = step
                    .stdin
                    .as_ref()
                    .map(|stdin| self.stdin(stdin, &format!("{path}.stdin")));
                if let Some(timeout) = step.timeout_seconds
                    && !(1..=init_seconds).contains(&timeout)
                {
                    self.at(
                        format!("{path}.timeoutSeconds"),
                        IssueCode::TimeoutOutOfRange,
                        format!(
                            "timeoutSeconds must be 1 to timeouts.initSeconds ({init_seconds})"
                        ),
                    );
                }
                let success = self.success_codes(
                    step.success_exit_codes.as_ref(),
                    &format!("{path}.successExitCodes"),
                );
                let stdin = match stdin {
                    Some(None) => return None,
                    Some(Some(spec)) => Some(spec),
                    None => None,
                };
                Some(InitStepSpec::RunExecutable {
                    command: command?,
                    stdin,
                    timeout: step.timeout_seconds,
                    success,
                })
            }
        }
    }

    fn stop(&mut self, raw: &RawManifest, env: &TypeEnv<'_>) -> Option<StopSpec> {
        let stop = &raw.lifecycle.stop;
        let path = "lifecycle.stop";
        match stop.strategy.as_str() {
            "gracefulProcess" => {
                self.reject_fields(
                    path,
                    "gracefulProcess",
                    &[
                        ("command", stop.command.is_some()),
                        ("successExitCodes", stop.success_exit_codes.is_some()),
                    ],
                );
                let signal = match stop.signal.as_deref() {
                    None | Some("term") => StopSignal::Term,
                    Some("int") => StopSignal::Int,
                    Some("quit") => StopSignal::Quit,
                    Some(_) => {
                        self.at(
                            format!("{path}.signal"),
                            IssueCode::InvalidValue,
                            "signal must be term, int, or quit",
                        );
                        return None;
                    }
                };
                Some(StopSpec::Signal(signal))
            }
            "command" => {
                self.reject_fields(
                    path,
                    "the command strategy",
                    &[("signal", stop.signal.is_some())],
                );
                let success = self.success_codes(
                    stop.success_exit_codes.as_ref(),
                    &format!("{path}.successExitCodes"),
                );
                let Some(command) = &stop.command else {
                    self.at(
                        format!("{path}.command"),
                        IssueCode::MissingField,
                        "the command strategy needs command",
                    );
                    return None;
                };
                let command = self.command(
                    &CommandParts::of(command),
                    &format!("{path}.command"),
                    env,
                    false,
                )?;
                Some(StopSpec::Command { command, success })
            }
            _ => {
                self.at(
                    format!("{path}.strategy"),
                    IssueCode::InvalidValue,
                    "strategy must be gracefulProcess or command",
                );
                None
            }
        }
    }

    fn reload(
        &mut self,
        raw: &RawManifest,
        env: &TypeEnv<'_>,
        config_files: &BTreeMap<String, ConfigFileSpec>,
    ) -> Option<ReloadSpec> {
        let path = "lifecycle.reload";
        let Some(reload) = &raw.lifecycle.reload else {
            if raw.capabilities.reload {
                self.at(
                    "capabilities.reload",
                    IssueCode::CapabilityUnsatisfiable,
                    "the reload capability needs lifecycle.reload",
                );
            }
            return None;
        };
        if !raw.capabilities.reload {
            self.at(
                path,
                IssueCode::ReloadWithoutCapability,
                "lifecycle.reload needs capabilities.reload: true",
            );
        }
        if reload.render.len() > RELOAD_RENDER_MAX {
            self.at(
                format!("{path}.render"),
                IssueCode::InvalidValue,
                format!("render lists at most {RELOAD_RENDER_MAX} config files"),
            );
        }
        for (index, name) in reload.render.iter().enumerate() {
            if !config_files.contains_key(name) {
                self.at(
                    format!("{path}.render[{index}]"),
                    IssueCode::UnknownConfigFile,
                    format!("configFiles has no {name}"),
                );
            }
        }
        let action = match reload.action.as_str() {
            "signal" => {
                self.reject_fields(
                    path,
                    "the signal action",
                    &[
                        ("command", reload.command.is_some()),
                        ("successExitCodes", reload.success_exit_codes.is_some()),
                    ],
                );
                match reload.signal.as_deref() {
                    Some("hup") => Some(ReloadActionSpec::Hangup),
                    None => {
                        self.at(
                            format!("{path}.signal"),
                            IssueCode::MissingField,
                            "the signal action needs signal: hup",
                        );
                        None
                    }
                    Some(_) => {
                        self.at(
                            format!("{path}.signal"),
                            IssueCode::InvalidValue,
                            "the reload signal is hup",
                        );
                        None
                    }
                }
            }
            "run" => {
                self.reject_fields(
                    path,
                    "the run action",
                    &[("signal", reload.signal.is_some())],
                );
                let success = self.success_codes(
                    reload.success_exit_codes.as_ref(),
                    &format!("{path}.successExitCodes"),
                );
                match &reload.command {
                    Some(command) => self
                        .command(
                            &CommandParts::of(command),
                            &format!("{path}.command"),
                            env,
                            false,
                        )
                        .map(|command| ReloadActionSpec::Run { command, success }),
                    None => {
                        self.at(
                            format!("{path}.command"),
                            IssueCode::MissingField,
                            "the run action needs command",
                        );
                        None
                    }
                }
            }
            "none" => {
                self.reject_fields(
                    path,
                    "the none action",
                    &[
                        ("signal", reload.signal.is_some()),
                        ("command", reload.command.is_some()),
                        ("successExitCodes", reload.success_exit_codes.is_some()),
                    ],
                );
                Some(ReloadActionSpec::None)
            }
            _ => {
                self.at(
                    format!("{path}.action"),
                    IssueCode::InvalidValue,
                    "action must be signal, run, or none",
                );
                None
            }
        };
        let verify = match reload.verify.as_deref() {
            None | Some("readiness") => true,
            Some("none") => false,
            Some(_) => {
                self.at(
                    format!("{path}.verify"),
                    IssueCode::InvalidValue,
                    "verify must be readiness or none",
                );
                true
            }
        };
        Some(ReloadSpec {
            render: reload.render.clone(),
            action: action?,
            verify,
        })
    }

    fn probe(&mut self, raw: &RawProbe, path: &str, env: &TypeEnv<'_>) -> Option<ProbeDef> {
        let endpoint = |checker: &mut Self,
                        accept: &[EndpointTransport],
                        kind: &str|
         -> Option<EndpointName> {
            let Some(name) = &raw.endpoint else {
                checker.at(
                    format!("{path}.endpoint"),
                    IssueCode::MissingField,
                    format!("{kind} probes need endpoint"),
                );
                return None;
            };
            let found = name
                .parse::<EndpointName>()
                .ok()
                .and_then(|name| env.endpoints.get(&name).map(|t| (name, *t)));
            match found {
                Some((name, transport)) if accept.contains(&transport) => Some(name),
                Some(_) => {
                    checker.at(
                        format!("{path}.endpoint"),
                        IssueCode::UnknownEndpoint,
                        format!("{kind} probes cannot use a {name} endpoint of that transport"),
                    );
                    None
                }
                None => {
                    checker.at(
                        format!("{path}.endpoint"),
                        IssueCode::UnknownEndpoint,
                        format!("no endpoint {name}"),
                    );
                    None
                }
            }
        };
        match raw.kind.as_str() {
            "tcp" => {
                self.reject_fields(
                    path,
                    "tcp probes",
                    &[
                        ("path", raw.path.is_some()),
                        ("expectStatus", raw.expect_status.is_some()),
                        ("command", raw.command.is_some()),
                        ("successExitCodes", raw.success_exit_codes.is_some()),
                    ],
                );
                endpoint(
                    self,
                    &[EndpointTransport::Tcp, EndpointTransport::Unix],
                    "tcp",
                )
                .map(|endpoint| ProbeDef::Tcp { endpoint })
            }
            "http" => {
                self.reject_fields(
                    path,
                    "http probes",
                    &[
                        ("command", raw.command.is_some()),
                        ("successExitCodes", raw.success_exit_codes.is_some()),
                    ],
                );
                let endpoint = endpoint(self, &[EndpointTransport::Tcp], "http");
                let probe_path = match &raw.path {
                    Some(text) => check_literal(text, &mut self.yaml(), &format!("{path}.path")).filter(|literal| {
                        let ok = literal.starts_with('/')
                            && literal.len() <= HTTP_PATH_MAX
                            && literal.bytes().all(|b| (0x21..=0x7e).contains(&b));
                        if !ok {
                            self.at(format!("{path}.path"), IssueCode::InvalidValue, "path starts with / and is at most 256 bytes of printable ASCII without spaces");
                        }
                        ok
                    }),
                    None => {
                        self.at(format!("{path}.path"), IssueCode::MissingField, "http probes need path");
                        None
                    }
                };
                let expect = match &raw.expect_status {
                    None => 200..=299,
                    Some(range)
                        if (100..=599).contains(&range.min)
                            && (100..=599).contains(&range.max)
                            && range.min <= range.max =>
                    {
                        range.min..=range.max
                    }
                    Some(_) => {
                        self.at(
                            format!("{path}.expectStatus"),
                            IssueCode::InvalidValue,
                            "expectStatus is min to max within 100 to 599",
                        );
                        return None;
                    }
                };
                Some(ProbeDef::Http {
                    endpoint: endpoint?,
                    path: probe_path?,
                    expect,
                })
            }
            "exec" => {
                self.reject_fields(
                    path,
                    "exec probes",
                    &[
                        ("endpoint", raw.endpoint.is_some()),
                        ("path", raw.path.is_some()),
                        ("expectStatus", raw.expect_status.is_some()),
                    ],
                );
                let success = self.success_codes(
                    raw.success_exit_codes.as_ref(),
                    &format!("{path}.successExitCodes"),
                );
                let Some(command) = &raw.command else {
                    self.at(
                        format!("{path}.command"),
                        IssueCode::MissingField,
                        "exec probes need command",
                    );
                    return None;
                };
                let command = self.command(
                    &CommandParts::of(command),
                    &format!("{path}.command"),
                    env,
                    false,
                )?;
                Some(ProbeDef::Exec { command, success })
            }
            other => {
                self.at(
                    format!("{path}.type"),
                    IssueCode::UnknownProbe,
                    format!("unknown probe {other}; use tcp, http, or exec"),
                );
                None
            }
        }
    }

    fn readiness(
        &mut self,
        raw: &RawManifest,
        env: &TypeEnv<'_>,
        timeouts: &Timeouts,
        reload: Option<&ReloadSpec>,
    ) -> Option<ReadinessSpec> {
        let block = &raw.readiness;
        let probe = self.probe(&block.probe, "readiness.probe", env);
        let interval_ms = self.bounded(
            block.interval_milliseconds,
            READINESS_INTERVAL_MS,
            "readiness.intervalMilliseconds",
        );
        let attempt_ms = self.bounded(
            block.attempt_timeout_milliseconds,
            ATTEMPT_TIMEOUT_MS,
            "readiness.attemptTimeoutMilliseconds",
        );
        let mut deadlines = vec![
            ("timeouts.readinessSeconds", timeouts.readiness),
            ("timeouts.recoverySeconds", timeouts.recovery),
        ];
        if reload.is_some_and(|reload| reload.verify) {
            deadlines.push(("timeouts.reloadSeconds", timeouts.reload));
        }
        for (deadline, seconds) in deadlines {
            if attempt_ms > seconds.saturating_mul(1000) {
                self.at(
                    "readiness.attemptTimeoutMilliseconds",
                    IssueCode::TimeoutOutOfRange,
                    format!("attemptTimeoutMilliseconds cannot exceed {deadline} ({seconds} s)"),
                );
            }
        }
        Some(ReadinessSpec {
            probe: probe?,
            interval_ms,
            attempt_ms,
        })
    }

    fn health(
        &mut self,
        raw: &RawManifest,
        env: &TypeEnv<'_>,
        readiness: Option<&ReadinessSpec>,
    ) -> Option<HealthSpec> {
        let block = raw.health.as_ref();
        let probe = match block.and_then(|block| block.probe.as_ref()) {
            Some(probe) => self.probe(probe, "health.probe", env),
            None => readiness.map(|readiness| readiness.probe.clone()),
        };
        let interval_seconds = self.bounded(
            block.and_then(|b| b.interval_seconds),
            HEALTH_INTERVAL_SECONDS,
            "health.intervalSeconds",
        );
        let attempt_ms = self.bounded(
            block.and_then(|b| b.attempt_timeout_milliseconds),
            ATTEMPT_TIMEOUT_MS,
            "health.attemptTimeoutMilliseconds",
        );
        let threshold = self.bounded(
            block.and_then(|b| b.failure_threshold),
            FAILURE_THRESHOLD,
            "health.failureThreshold",
        );
        Some(HealthSpec {
            probe: probe?,
            interval_seconds,
            attempt_ms,
            failure_threshold: u8::try_from(threshold).unwrap_or(1),
        })
    }

    fn log_files(&mut self, raw: &RawManifest, env: &TypeEnv<'_>) -> Vec<PathExpr> {
        let Some(logs) = &raw.logs else {
            return Vec::new();
        };
        if logs.files.len() > LOG_FILES_MAX {
            self.at(
                "logs.files",
                IssueCode::InvalidValue,
                format!("at most {LOG_FILES_MAX} log files"),
            );
        }
        logs.files
            .iter()
            .enumerate()
            .filter_map(|(index, text)| {
                check_path(
                    text,
                    PathPosition::LogFile,
                    env,
                    &mut self.yaml(),
                    &format!("logs.files[{index}]"),
                )
            })
            .collect()
    }

    fn capabilities(
        &mut self,
        raw: &RawManifest,
        endpoints: &BTreeMap<EndpointName, EndpointSpec>,
        managed_fs: &BTreeSet<InstanceDir>,
        has_reload: bool,
    ) -> (CapabilitySet, Vec<InstanceDir>) {
        let caps = &raw.capabilities;
        let unsatisfiable = IssueCode::CapabilityUnsatisfiable;
        let mut list = Vec::new();
        if caps.reload && has_reload {
            list.push(Capability::Reload);
        }
        if caps.metrics {
            if endpoints
                .values()
                .any(|endpoint| endpoint.purpose == EndpointPurpose::Monitoring)
            {
                list.push(Capability::Metrics);
            } else {
                self.at(
                    "capabilities.metrics",
                    unsatisfiable,
                    "metrics needs an endpoint with purpose monitoring",
                );
            }
        }
        if caps.tls {
            self.at(
                "capabilities.tls",
                unsatisfiable,
                "tls has no primitive in stackly/v1alpha1",
            );
        }
        if caps.multi_instance {
            list.push(Capability::MultiInstance);
        }
        if caps.persistence {
            list.push(Capability::Persistence);
        }
        let cold_paths = raw.backup.as_ref().and_then(|backup| backup.cold.as_ref());
        let mut cold = false;
        let mut seen = BTreeSet::new();
        for (index, kind) in caps.backup.iter().enumerate() {
            let path = format!("capabilities.backup[{index}]");
            if !seen.insert(kind.as_str()) {
                self.at(
                    &path,
                    IssueCode::InvalidValue,
                    "each backup kind appears once",
                );
                continue;
            }
            match kind.as_str() {
                "cold" => {
                    cold = true;
                    if !caps.persistence {
                        self.at(&path, unsatisfiable, "cold backup needs persistence: true");
                    } else if cold_paths.is_none() {
                        self.at(&path, unsatisfiable, "cold backup needs backup.cold.paths");
                    } else {
                        list.push(Capability::ColdBackup);
                    }
                }
                "online" | "logical" => self.at(
                    &path,
                    unsatisfiable,
                    format!("{kind} backup has no primitive in stackly/v1alpha1"),
                ),
                _ => self.at(
                    &path,
                    IssueCode::InvalidValue,
                    "backup kinds are cold, online, or logical",
                ),
            }
        }
        let mut paths = Vec::new();
        if let Some(cold_paths) = cold_paths {
            if !cold {
                self.at(
                    "backup.cold",
                    unsatisfiable,
                    "backup.cold needs backup: [cold] in capabilities",
                );
            }
            for (index, code) in cold_paths.paths.iter().enumerate() {
                let path = format!("backup.cold.paths[{index}]");
                match InstanceDir::from_code(code) {
                    Some(dir @ (InstanceDir::Data | InstanceDir::Config))
                        if managed_fs.contains(&dir) =>
                    {
                        paths.push(dir)
                    }
                    Some(InstanceDir::Data | InstanceDir::Config) => {
                        self.at(
                            path,
                            unsatisfiable,
                            "a backup path must be listed in permissions.managedFilesystem",
                        );
                    }
                    _ => self.at(
                        path,
                        IssueCode::InvalidValue,
                        "backup paths are data or config",
                    ),
                }
            }
            if cold_paths.paths.is_empty() {
                self.at(
                    "backup.cold.paths",
                    unsatisfiable,
                    "list at least one backup path",
                );
            }
        }
        (CapabilitySet::new(list), paths)
    }
}

/// Whether a manifest default host is one of the two loopback addresses
/// the spec allows.
fn is_loopback_default(host: IpAddr) -> bool {
    host == IpAddr::from([127, 0, 0, 1]) || host == IpAddr::from(std::net::Ipv6Addr::LOCALHOST)
}

fn text_ok(text: &str, min: usize, max: usize) -> bool {
    let len = text.chars().count();
    (min..=max).contains(&len) && !text.chars().any(char::is_control)
}

/// Checks every referenced template and reports unreferenced ones.
fn check_templates(
    manifest: &Manifest,
    settings: Option<&SettingsSchema>,
    parsed: &BTreeMap<String, template::Template>,
    names: &BTreeSet<String>,
    issues: &mut Issues,
) -> Option<BTreeMap<String, Checked>> {
    let referenced: BTreeSet<&String> = manifest
        .config_files
        .values()
        .map(|file| &file.template)
        .collect();
    for name in names {
        if !referenced.contains(name) {
            issues.push(Issue::new(
                format!("templates/{name}"),
                Location::File,
                IssueCode::UnreferencedTemplate,
                "no config file uses this template",
            ));
        }
    }
    let transports: BTreeMap<EndpointName, EndpointTransport> = manifest
        .endpoints
        .iter()
        .map(|(name, spec)| (name.clone(), spec.transport))
        .collect();
    let aliases: BTreeSet<String> = manifest.aliases.keys().cloned().collect();
    let env = TypeEnv {
        endpoints: &transports,
        aliases: &aliases,
        settings,
        managed_fs: &manifest.managed_fs,
    };
    let start = issues.count();
    let mut checked = BTreeMap::new();
    for name in referenced {
        let file = format!("templates/{name}");
        let Some(template) = parsed.get(name) else {
            continue;
        };
        if let Some(result) = template::check(template, &env, &mut FileIssues::new(&file, issues)) {
            checked.insert(name.clone(), result);
        }
    }
    for (config, spec) in &manifest.config_files {
        let Some(result) = checked.get(&spec.template) else {
            continue;
        };
        let file = format!("templates/{}", spec.template);
        if !spec.format.allows_tags() {
            for at in result.tag_positions() {
                issues.push(Issue::new(
                    &file,
                    Location::Position {
                        line: at.line,
                        column: at.column,
                    },
                    IssueCode::TemplateType,
                    format!("config file {config} is plain, which allows only text and comments"),
                ));
            }
        }
        if spec.sensitive {
            continue;
        }
        for (secret, at) in result.secrets() {
            issues.push(Issue::new(
                &file,
                Location::Position {
                    line: at.line,
                    column: at.column,
                },
                IssueCode::SecretNotAllowedHere,
                format!("secret.{secret} needs config file {config} to be sensitive"),
            ));
        }
    }
    (issues.count() == start && settings.is_some()).then_some(checked)
}

/// Checks the `apply: reload` rule: such a field is referenced only from
/// templates of config files that `lifecycle.reload.render` re-renders,
/// and only with the reload capability.
fn check_setting_refs(
    manifest: &Manifest,
    settings: Option<&SettingsSchema>,
    refs: &[(SettingName, RefSite)],
    templates: &Option<BTreeMap<String, Checked>>,
    issues: &mut Issues,
) {
    let Some(settings) = settings else {
        return;
    };
    let mut all = refs.to_vec();
    if let Some(templates) = templates {
        for (name, checked) in templates {
            for (token, _) in &checked.tokens {
                if let Token::Setting(setting) = token {
                    all.push((setting.clone(), RefSite::Template(name.clone())));
                }
            }
        }
    }
    let rendered: BTreeSet<&String> = manifest
        .reload
        .iter()
        .flat_map(|reload| reload.render.iter())
        .collect();
    let mut file = FileIssues::new(SETTINGS_ISSUE_FILE, issues);
    for field in settings.fields() {
        if field.apply() != Some(ApplyClass::Reload) {
            continue;
        }
        let path = format!("properties.{}.x-stackly.apply", field.name());
        if !manifest.capabilities.contains(Capability::Reload) {
            file.at(
                &path,
                IssueCode::CapabilityUnsatisfiable,
                "apply: reload needs the reload capability",
            );
            continue;
        }
        for (name, site) in &all {
            if name != field.name() {
                continue;
            }
            let reaches = match site {
                RefSite::Manifest(_) => false,
                RefSite::Template(template) => manifest
                    .config_files
                    .iter()
                    .filter(|(_, spec)| &spec.template == template)
                    .all(|(config, _)| rendered.contains(config)),
            };
            if !reaches {
                let place = match site {
                    RefSite::Manifest(at) => at.clone(),
                    RefSite::Template(template) => format!("templates/{template}"),
                };
                file.at(
                    &path,
                    IssueCode::SettingsSchemaRule,
                    format!(
                        "{} applies by reload but {place} is not re-rendered by lifecycle.reload",
                        field.name()
                    ),
                );
            }
        }
    }
}

/// The context of validation renders: fixed sample paths and endpoint
/// defaults.
pub(crate) fn sample_context(
    manifest: &Manifest,
    settings: &SettingsSchema,
    with_items: bool,
) -> Option<PlanContext> {
    let base = PathBuf::from(if cfg!(windows) {
        r"C:\stackly"
    } else {
        "/stackly"
    });
    let roots = RootSet::new(
        base.join("data"),
        base.join("logs"),
        base.join("cache"),
        base.join("run"),
    )
    .ok()?;
    let id: InstanceId = "0192f000-0000-7000-8000-000000000001".parse().ok()?;
    let endpoints = manifest
        .endpoints
        .iter()
        .map(|(name, spec)| {
            let resolved = match (spec.default_host, spec.default_port) {
                (Some(host), port) => ResolvedEndpoint::Ip { host, port },
                _ => ResolvedEndpoint::Unix,
            };
            (name.clone(), resolved)
        })
        .collect();
    Some(PlanContext {
        instance_id: id,
        instance_name: "sample".parse::<InstanceName>().ok()?,
        paths: DirectoryLayout::new(roots).instance_paths(id),
        runtime: RuntimeContext {
            root: base.join("runtime"),
            version: "0.0".parse().ok()?,
            line: "0".parse().ok()?,
        },
        endpoints,
        settings: settings.sample_values(with_items),
    })
}

/// Renders every config file with sample values, once with empty lists
/// and once with one sample item each, and re-parses `yaml` and `json`.
fn sample_renders(
    manifest: &Manifest,
    settings: &SettingsSchema,
    templates: &BTreeMap<String, Checked>,
    issues: &mut Issues,
) {
    for with_items in [false, true] {
        let Some(context) = sample_context(manifest, settings, with_items) else {
            return;
        };
        let resolver = match super::plan::Resolver::new(manifest, &context) {
            Ok(resolver) => resolver,
            Err(err) => {
                issues.push(Issue::new(
                    MANIFEST_FILE,
                    Location::File,
                    IssueCode::InvalidValue,
                    format!("sample values do not resolve: {err}"),
                ));
                return;
            }
        };
        for (config, spec) in &manifest.config_files {
            let Some(checked) = templates.get(&spec.template) else {
                continue;
            };
            let file = format!("templates/{}", spec.template);
            let outcome = template::render(checked, spec.format, &resolver, config)
                .and_then(|content| content.materialize_sample())
                .and_then(|bytes| {
                    let text = String::from_utf8_lossy(&bytes);
                    if spec.format.output_parses(&text) {
                        Ok(())
                    } else {
                        Err(super::plan::PlanError::RenderedOutputInvalid {
                            config_file: config.clone(),
                        })
                    }
                });
            if let Err(err) = outcome {
                let code = match err {
                    super::plan::PlanError::UnrepresentableValue { .. } => {
                        IssueCode::UnrepresentableValue
                    }
                    super::plan::PlanError::OutputTooLarge { .. } => IssueCode::OutputTooLarge,
                    super::plan::PlanError::RenderedOutputInvalid { .. } => {
                        IssueCode::RenderedOutputInvalid
                    }
                    _ => IssueCode::InvalidValue,
                };
                let which = if with_items {
                    "one sample item per list"
                } else {
                    "empty lists"
                };
                issues.push(Issue::new(
                    file,
                    Location::File,
                    code,
                    format!("sample render of {config} with {which}: {err}"),
                ));
            }
        }
    }
}
