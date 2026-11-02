//! The manifest `openWith` block (spec 0070): actions that open an
//! instance in another application, a client app, the runtime's CLI in
//! Terminal, a page in the browser, or a config file in Finder.
//!
//! The block is checked when the package loads: names, titles, the kind
//! tables, lengths, env names, bundle IDs, references, and the tokens of the
//! `openWith` positions, which take the connection template tokens without
//! `secret.<field>`. A secret can therefore never reach a URL, an argument,
//! or an env value; a password reaches the user only through the hidden
//! clipboard of the GUI (`copySecret`). URLs are rendered with sentinel
//! values once more and must give an absolute RFC 3986 URI (`http` or
//! `https` for a browser).

use std::collections::{BTreeMap, BTreeSet};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::path::PathBuf;

use stackly_domain::Port;

use super::connection::{RenderFailure, encode_value, is_absolute_uri, is_inserted_raw};
use super::interpolate::{InstanceDir, Interp, Piece, TypeEnv, ValuePosition, check_value};
use super::issue::{FileIssues, Issue, IssueCode, Issues, Location};
use super::manifest::Manifest;
use super::names::{SettingName, env_key_problem};
use super::package::MANIFEST_FILE;
use super::plan::{PlanContext, PlanError, ResolvedEndpoint, Resolver, Value};
use super::raw::{RawConfigFile, RawOpenWith};
use super::settings::SettingsSchema;
use super::validate::sample_context;

/// Most entries one `openWith` block holds.
pub const OPEN_WITH_ENTRIES_MAX: usize = 8;
/// Longest `title`, in characters.
pub const OPEN_WITH_TITLE_MAX: usize = 48;
/// Longest `url`, in bytes.
pub const OPEN_WITH_URL_MAX: usize = 512;
/// Most bundle IDs of an `app` entry.
pub const OPEN_WITH_BUNDLE_IDS_MAX: usize = 4;
/// Most `args` of a `terminal` entry.
pub const OPEN_WITH_ARGS_MAX: usize = 32;
/// Most env pairs of a `terminal` entry.
pub const OPEN_WITH_ENV_MAX: usize = 16;
/// Longest arg or env value of a `terminal` entry, in bytes.
pub const OPEN_WITH_VALUE_MAX: usize = 1024;
/// Longest `name`, in bytes.
const OPEN_WITH_NAME_MAX: usize = 32;
/// Longest bundle ID, in bytes.
const BUNDLE_ID_MAX: usize = 255;

/// The reasons of an `open_with_invalid` issue, carried in `Issue::reason`.
pub(crate) mod reason {
    /// The name breaks `^[a-z][a-z0-9_]{0,31}$`.
    pub(crate) const NAME: &str = "name";
    /// The title is empty, too long, or holds a control character.
    pub(crate) const TITLE: &str = "title";
    /// `kind` is not `app`, `terminal`, `browser`, or `reveal`.
    pub(crate) const KIND: &str = "kind";
    /// The kind tables do not match `kind`: the one it names is missing, or
    /// another one is set.
    pub(crate) const KIND_TABLE: &str = "kind_table";
    /// A URL is empty or longer than 512 bytes.
    pub(crate) const URL_LENGTH: &str = "url_length";
    /// The bundle ID list is empty, too long, or holds an ID that is not a
    /// bundle ID.
    pub(crate) const BUNDLE_ID_INVALID: &str = "bundle_id_invalid";
    /// With sentinel values the URL does not render to an absolute URI.
    pub(crate) const NOT_A_URI: &str = "not_a_uri";
    /// A browser URL whose scheme is not `http` or `https`.
    pub(crate) const NOT_HTTP: &str = "not_http";
    /// Too many args or env pairs, or an arg or env value over 1024 bytes.
    pub(crate) const TOO_LONG: &str = "too_long";
    /// An env name outside the env rule of spec 0009.
    pub(crate) const ENV_NAME_INVALID: &str = "env_name_invalid";
    /// `copySecret` on a `browser` or `reveal` entry.
    pub(crate) const COPY_SECRET_NOT_ALLOWED: &str = "copy_secret_not_allowed";
    /// `reveal.configFile` is not a key of `configFiles`.
    pub(crate) const UNKNOWN_CONFIG_FILE: &str = "unknown_config_file";
    /// `reveal.configFile` names a sensitive file.
    pub(crate) const CONFIG_FILE_SENSITIVE: &str = "config_file_sensitive";
}

/// The port every IP endpoint gets in the sentinel render (spec 0021).
const SENTINEL_PORT: u16 = 15432;
/// The hosts the sentinel render tries, IPv4 and IPv6 loopback.
const SENTINEL_HOSTS: [IpAddr; 2] = [
    IpAddr::V4(Ipv4Addr::LOCALHOST),
    IpAddr::V6(Ipv6Addr::LOCALHOST),
];
/// The run folder of the sentinel render, as in spec 0021.
const SENTINEL_RUN_DIR: &str = if cfg!(windows) {
    r"C:\stackly sentinel\run"
} else {
    "/stackly sentinel/run"
};
/// The runtime root of the sentinel render, so `runtime.executable` and
/// `runtime.executables.<alias>` become absolute paths under it.
const SENTINEL_RUNTIME_DIR: &str = if cfg!(windows) {
    r"C:\stackly sentinel\runtime"
} else {
    "/stackly sentinel/runtime"
};
/// The runtime version of the sentinel render.
const SENTINEL_RUNTIME_VERSION: &str = "0.0.0";

/// One checked `openWith` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenWithSpec {
    pub(crate) name: String,
    pub(crate) title: String,
    pub(crate) action: OpenWithAction,
    pub(crate) copy_secret: Option<SettingName>,
}

impl OpenWithSpec {
    /// Returns the entry name, unique in the manifest.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the title the GUI puts in the label.
    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Returns the kind of the entry.
    #[must_use]
    pub const fn kind(&self) -> OpenWithKind {
        match self.action {
            OpenWithAction::App { .. } => OpenWithKind::App,
            OpenWithAction::Terminal { .. } => OpenWithKind::Terminal,
            OpenWithAction::Browser { .. } => OpenWithKind::Browser,
            OpenWithAction::Reveal { .. } => OpenWithKind::Reveal,
        }
    }
}

/// The closed set of `openWith` kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum OpenWithKind {
    /// Opens a URL with a client app chosen by bundle ID.
    App,
    /// Runs a runtime executable in Terminal.
    Terminal,
    /// Opens an `http` or `https` URL in the default browser.
    Browser,
    /// Shows a rendered config file in Finder.
    Reveal,
}

impl OpenWithKind {
    /// Every kind, in manifest code order.
    pub const ALL: [Self; 4] = [Self::App, Self::Terminal, Self::Browser, Self::Reveal];

    /// Returns the manifest code, such as `app`.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::App => "app",
            Self::Terminal => "terminal",
            Self::Browser => "browser",
            Self::Reveal => "reveal",
        }
    }

    fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_code() == code)
    }
}

/// The checked table of an entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OpenWithAction {
    App {
        bundle_ids: Vec<String>,
        url: Interp,
    },
    Terminal {
        executable: Interp,
        args: Vec<Interp>,
        env: Vec<(String, Interp)>,
    },
    Browser {
        url: Interp,
    },
    Reveal {
        config_file: String,
    },
}

/// Checks the `openWith` block and returns its entries in manifest order.
/// A package without the block has no entries.
pub(crate) fn check(
    raw: Option<&[RawOpenWith]>,
    config_files: &BTreeMap<String, RawConfigFile>,
    env: &TypeEnv<'_>,
    issues: &mut FileIssues<'_>,
) -> Vec<OpenWithSpec> {
    let Some(raw) = raw else {
        return Vec::new();
    };
    if raw.is_empty() || raw.len() > OPEN_WITH_ENTRIES_MAX {
        issues.at(
            "openWith",
            IssueCode::InvalidValue,
            format!("openWith holds 1 to {OPEN_WITH_ENTRIES_MAX} entries"),
        );
    }
    let mut seen = BTreeSet::new();
    let mut entries = Vec::new();
    for (index, entry) in raw.iter().enumerate() {
        let start = issues.count();
        let path = format!("openWith[{index}]");
        if !is_open_name(&entry.name) {
            issues.at_with_reason(
                format!("{path}.name"),
                IssueCode::OpenWithInvalid,
                reason::NAME,
                "name must be a lowercase letter, then at most 31 lowercase letters, digits, or _",
            );
        } else if !seen.insert(entry.name.as_str()) {
            issues.at(
                format!("{path}.name"),
                IssueCode::OpenWithNameDuplicate,
                format!("the openWith name {} is used twice", entry.name),
            );
        }
        let title_len = entry.title.chars().count();
        if !(1..=OPEN_WITH_TITLE_MAX).contains(&title_len)
            || entry.title.chars().any(char::is_control)
        {
            issues.at_with_reason(
                format!("{path}.title"),
                IssueCode::OpenWithInvalid,
                reason::TITLE,
                format!(
                    "title must be 1 to {OPEN_WITH_TITLE_MAX} characters without control characters"
                ),
            );
        }
        let action = action(entry, &path, config_files, env, issues);
        let copy_secret = copy_secret(entry, &path, env, issues);
        if let Some(action) = action
            && issues.count() == start
        {
            entries.push(OpenWithSpec {
                name: entry.name.clone(),
                title: entry.title.clone(),
                action,
                copy_secret,
            });
        }
    }
    entries
}

/// Checks `kind` and the one table it names.
fn action(
    entry: &RawOpenWith,
    path: &str,
    config_files: &BTreeMap<String, RawConfigFile>,
    env: &TypeEnv<'_>,
    issues: &mut FileIssues<'_>,
) -> Option<OpenWithAction> {
    let Some(kind) = OpenWithKind::from_code(&entry.kind) else {
        issues.at_with_reason(
            format!("{path}.kind"),
            IssueCode::OpenWithInvalid,
            reason::KIND,
            "kind must be app, terminal, browser, or reveal",
        );
        return None;
    };
    let set = [
        (OpenWithKind::App, entry.app.is_some()),
        (OpenWithKind::Terminal, entry.terminal.is_some()),
        (OpenWithKind::Browser, entry.browser.is_some()),
        (OpenWithKind::Reveal, entry.reveal.is_some()),
    ];
    if set
        .iter()
        .any(|(table, present)| *present != (*table == kind))
    {
        issues.at_with_reason(
            path,
            IssueCode::OpenWithInvalid,
            reason::KIND_TABLE,
            format!(
                "an entry of kind {} sets exactly the {} table",
                kind.as_code(),
                kind.as_code()
            ),
        );
        return None;
    }
    match (&entry.app, &entry.terminal, &entry.browser, &entry.reveal) {
        (Some(app), _, _, _) => {
            let table = format!("{path}.app");
            let bundles_ok = (1..=OPEN_WITH_BUNDLE_IDS_MAX).contains(&app.bundle_ids.len())
                && app.bundle_ids.iter().all(|id| is_bundle_id(id));
            if !bundles_ok {
                issues.at_with_reason(
                    format!("{table}.bundleIds"),
                    IssueCode::OpenWithInvalid,
                    reason::BUNDLE_ID_INVALID,
                    format!(
                        "bundleIds lists 1 to {OPEN_WITH_BUNDLE_IDS_MAX} bundle IDs such as com.example.App"
                    ),
                );
            }
            let url = url(&app.url, &format!("{table}.url"), env, issues)?;
            bundles_ok.then(|| OpenWithAction::App {
                bundle_ids: app.bundle_ids.clone(),
                url,
            })
        }
        (_, Some(terminal), _, _) => {
            let table = format!("{path}.terminal");
            if !env.managed_fs.contains(&InstanceDir::Temp) {
                issues.at(
                    table.clone(),
                    IssueCode::PathNotPermitted,
                    "a terminal entry writes its script to the temp folder, which permissions.managedFilesystem does not list",
                );
            }
            let executable = check_value(
                &terminal.executable,
                ValuePosition::OpenExecutable,
                env,
                issues,
                &format!("{table}.executable"),
            );
            if terminal.args.len() > OPEN_WITH_ARGS_MAX {
                issues.at_with_reason(
                    format!("{table}.args"),
                    IssueCode::OpenWithInvalid,
                    reason::TOO_LONG,
                    format!("a terminal entry has at most {OPEN_WITH_ARGS_MAX} args"),
                );
            }
            let mut ok = executable.is_some();
            let mut args = Vec::with_capacity(terminal.args.len());
            for (index, arg) in terminal.args.iter().enumerate() {
                let at = format!("{table}.args[{index}]");
                if arg.len() > OPEN_WITH_VALUE_MAX {
                    too_long_value(issues, at);
                    ok = false;
                    continue;
                }
                match check_value(arg, ValuePosition::OpenArg, env, issues, &at) {
                    Some(arg) => args.push(arg),
                    None => ok = false,
                }
            }
            if terminal.env.len() > OPEN_WITH_ENV_MAX {
                issues.at_with_reason(
                    format!("{table}.env"),
                    IssueCode::OpenWithInvalid,
                    reason::TOO_LONG,
                    format!("a terminal entry has at most {OPEN_WITH_ENV_MAX} env pairs"),
                );
            }
            let mut env_pairs = Vec::with_capacity(terminal.env.len());
            for (key, value) in &terminal.env {
                let at = format!("{table}.env.{key}");
                if let Some(problem) = env_key_problem(key) {
                    issues.at_with_reason(
                        at,
                        IssueCode::OpenWithInvalid,
                        reason::ENV_NAME_INVALID,
                        problem,
                    );
                    ok = false;
                    continue;
                }
                if value.len() > OPEN_WITH_VALUE_MAX {
                    too_long_value(issues, at);
                    ok = false;
                    continue;
                }
                match check_value(value, ValuePosition::OpenEnv, env, issues, &at) {
                    Some(value) => env_pairs.push((key.clone(), value)),
                    None => ok = false,
                }
            }
            ok.then(|| OpenWithAction::Terminal {
                executable: executable.unwrap_or_default(),
                args,
                env: env_pairs,
            })
        }
        (_, _, Some(browser), _) => url(&browser.url, &format!("{path}.browser.url"), env, issues)
            .map(|url| OpenWithAction::Browser { url }),
        (_, _, _, Some(reveal)) => {
            let at = format!("{path}.reveal.configFile");
            match config_files.get(&reveal.config_file) {
                None => {
                    issues.at_with_reason(
                        at,
                        IssueCode::OpenWithInvalid,
                        reason::UNKNOWN_CONFIG_FILE,
                        format!("configFiles has no file {}", reveal.config_file),
                    );
                    None
                }
                Some(file) if file.sensitive => {
                    issues.at_with_reason(
                        at,
                        IssueCode::OpenWithInvalid,
                        reason::CONFIG_FILE_SENSITIVE,
                        format!(
                            "the config file {} is sensitive and cannot be revealed",
                            reveal.config_file
                        ),
                    );
                    None
                }
                Some(_) => Some(OpenWithAction::Reveal {
                    config_file: reveal.config_file.clone(),
                }),
            }
        }
        (None, None, None, None) => None,
    }
}

/// Reports an arg or env value over the length limit.
fn too_long_value(issues: &mut FileIssues<'_>, at: String) {
    issues.at_with_reason(
        at,
        IssueCode::OpenWithInvalid,
        reason::TOO_LONG,
        format!("an arg or env value is at most {OPEN_WITH_VALUE_MAX} bytes"),
    );
}

/// Checks a URL template: its length, then its tokens.
fn url(text: &str, at: &str, env: &TypeEnv<'_>, issues: &mut FileIssues<'_>) -> Option<Interp> {
    if !(1..=OPEN_WITH_URL_MAX).contains(&text.len()) {
        issues.at_with_reason(
            at,
            IssueCode::OpenWithInvalid,
            reason::URL_LENGTH,
            format!("url must be 1 to {OPEN_WITH_URL_MAX} bytes"),
        );
        return None;
    }
    check_value(text, ValuePosition::OpenUrl, env, issues, at)
}

/// Checks `copySecret`: allowed on `app` and `terminal` only, and it names
/// a secret field of the settings schema.
fn copy_secret(
    entry: &RawOpenWith,
    path: &str,
    env: &TypeEnv<'_>,
    issues: &mut FileIssues<'_>,
) -> Option<SettingName> {
    let field = entry.copy_secret.as_deref()?;
    let at = format!("{path}.copySecret");
    if matches!(entry.kind.as_str(), "browser" | "reveal") {
        issues.at_with_reason(
            at,
            IssueCode::OpenWithInvalid,
            reason::COPY_SECRET_NOT_ALLOWED,
            "copySecret is allowed only on app and terminal entries",
        );
        return None;
    }
    // A schema that did not decode reports its own issue.
    let schema = env.settings?;
    match schema.field(field) {
        Some(spec) if spec.is_secret() => Some(spec.name().clone()),
        _ => {
            issues.at(
                at,
                IssueCode::UnknownSecret,
                format!("the settings schema has no secret field {field}"),
            );
            None
        }
    }
}

/// Whether `name` matches `^[a-z][a-z0-9_]{0,31}$`.
fn is_open_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    matches!(bytes.first(), Some(b'a'..=b'z'))
        && bytes.len() <= OPEN_WITH_NAME_MAX
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'_')
}

/// Whether `id` is a bundle ID: at least two dot separated parts of ASCII
/// letters, digits, and `-`, at most 255 bytes.
fn is_bundle_id(id: &str) -> bool {
    id.len() <= BUNDLE_ID_MAX
        && id.split('.').count() >= 2
        && id.split('.').all(|part| {
            !part.is_empty() && part.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
}

/// Renders every `openWith` URL of a valid manifest with sentinel values,
/// once with each loopback host, and reports a URL that is not an absolute
/// RFC 3986 URI (`not_a_uri`), or a browser URL that is not `http` or
/// `https` (`not_http`).
pub(crate) fn check_sentinels(manifest: &Manifest, settings: &SettingsSchema, issues: &mut Issues) {
    if manifest.open_with.is_empty() {
        return;
    }
    let mut reported = BTreeSet::new();
    for host in SENTINEL_HOSTS {
        let Some(context) = sentinel_context(manifest, settings, host) else {
            return;
        };
        let resolver = match Resolver::without_managed(manifest, &context) {
            Ok(resolver) => resolver,
            Err(err) => {
                issues.push(Issue::new(
                    MANIFEST_FILE,
                    Location::Field("openWith".to_owned()),
                    IssueCode::InvalidValue,
                    format!("sentinel values do not resolve: {err}"),
                ));
                return;
            }
        };
        let mut yaml = FileIssues::new(MANIFEST_FILE, issues);
        for (index, entry) in manifest.open_with.iter().enumerate() {
            if reported.contains(&index) {
                continue;
            }
            let (url, at, browser) = match &entry.action {
                OpenWithAction::App { url, .. } => {
                    (url, format!("openWith[{index}].app.url"), false)
                }
                OpenWithAction::Browser { url } => {
                    (url, format!("openWith[{index}].browser.url"), true)
                }
                OpenWithAction::Terminal { .. } | OpenWithAction::Reveal { .. } => continue,
            };
            let problem = match render_url(url, &resolver) {
                Ok(text) if !is_absolute_uri(&text) => Some((
                    reason::NOT_A_URI,
                    format!(
                        "with sentinel values and host {host} the url does not render to an absolute uri (RFC 3986)"
                    ),
                )),
                Ok(text) if browser && !is_http(&text) => Some((
                    reason::NOT_HTTP,
                    "a browser url must use the http or https scheme".to_owned(),
                )),
                Ok(_) => None,
                Err(err) => {
                    reported.insert(index);
                    yaml.at(
                        at,
                        IssueCode::InvalidValue,
                        format!("the url does not render with sentinel values: {err}"),
                    );
                    continue;
                }
            };
            if let Some((reason, message)) = problem {
                reported.insert(index);
                yaml.at_with_reason(at, IssueCode::OpenWithInvalid, reason, message);
            }
        }
    }
}

/// The sample context with the sentinel values of spec 0021 and the
/// sentinel runtime of spec 0070.
fn sentinel_context(
    manifest: &Manifest,
    settings: &SettingsSchema,
    host: IpAddr,
) -> Option<PlanContext> {
    let mut context = sample_context(manifest, settings, false)?;
    let port = Port::new(SENTINEL_PORT).ok();
    for endpoint in context.endpoints.values_mut() {
        if let ResolvedEndpoint::Ip { .. } = endpoint {
            *endpoint = ResolvedEndpoint::Ip { host, port };
        }
    }
    context.paths.run = PathBuf::from(SENTINEL_RUN_DIR);
    context.runtime.root = PathBuf::from(SENTINEL_RUNTIME_DIR);
    context.runtime.version = SENTINEL_RUNTIME_VERSION.parse().ok()?;
    Some(context)
}

/// Whether a URI uses the `http` or `https` scheme, in any case.
fn is_http(text: &str) -> bool {
    text.split_once(':').is_some_and(|(scheme, _)| {
        scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https")
    })
}

/// Renders a URL template: every token value percent encoded except
/// endpoint ports and addresses (spec 0021, AC-2), literal text as written.
fn render_url(url: &Interp, resolver: &Resolver<'_>) -> Result<String, PlanError> {
    let mut out = String::new();
    for piece in &url.pieces {
        match piece {
            Piece::Literal(text) => out.push_str(text),
            Piece::Token(token) => match resolver.value(token)? {
                Value::Text(text) if is_inserted_raw(token) => out.push_str(&text),
                Value::Text(text) => out.push_str(&encode_value(&text)),
                Value::Integer(number) => out.push_str(&number.to_string()),
                Value::Boolean(flag) => out.push_str(if flag { "true" } else { "false" }),
                Value::Secret(_) => {
                    return Err(PlanError::UnrepresentableValue {
                        field: token.to_string(),
                    });
                }
            },
        }
    }
    Ok(out)
}

/// The rendered value of one `openWith` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RenderedOpenValue {
    /// An `app` entry: the bundle IDs in priority order and the URL.
    App {
        bundle_ids: Vec<String>,
        url: String,
    },
    /// A `terminal` entry: the absolute executable, args, and env pairs,
    /// every value inserted as is.
    Terminal {
        executable: String,
        args: Vec<String>,
        env: Vec<(String, String)>,
    },
    /// A `browser` entry: the URL.
    Browser { url: String },
    /// A `reveal` entry: the absolute path of the config file.
    Reveal { path: PathBuf },
}

/// One entry of the `openWith` block after a render.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RenderedOpenEntry {
    pub(crate) name: String,
    pub(crate) title: String,
    pub(crate) kind: OpenWithKind,
    pub(crate) copy_secret: Option<SettingName>,
    pub(crate) value: Result<RenderedOpenValue, RenderFailure>,
}

/// Renders every `openWith` entry of the manifest against `context`, in
/// manifest order, without settings (an entry cannot name one). Nothing is
/// read from the Keychain or the filesystem.
pub(crate) fn render_open_entries(
    manifest: &Manifest,
    context: &PlanContext,
) -> Vec<RenderedOpenEntry> {
    let resolver = Resolver::without_managed(manifest, context);
    manifest
        .open_with
        .iter()
        .map(|spec| RenderedOpenEntry {
            name: spec.name.clone(),
            title: spec.title.clone(),
            kind: spec.kind(),
            copy_secret: spec.copy_secret.clone(),
            value: match &resolver {
                Ok(resolver) => {
                    render_value(spec, manifest, resolver).map_err(|err| RenderFailure::of(&err))
                }
                Err(err) => Err(RenderFailure::of(err)),
            },
        })
        .collect()
}

/// Renders the table of one entry.
fn render_value(
    spec: &OpenWithSpec,
    manifest: &Manifest,
    resolver: &Resolver<'_>,
) -> Result<RenderedOpenValue, PlanError> {
    Ok(match &spec.action {
        OpenWithAction::App { bundle_ids, url } => RenderedOpenValue::App {
            bundle_ids: bundle_ids.clone(),
            url: render_url(url, resolver)?,
        },
        OpenWithAction::Browser { url } => RenderedOpenValue::Browser {
            url: render_url(url, resolver)?,
        },
        OpenWithAction::Terminal {
            executable,
            args,
            env,
        } => {
            let at = format!("openWith.{}", spec.name);
            RenderedOpenValue::Terminal {
                executable: resolver.render(executable, &at)?,
                args: args
                    .iter()
                    .map(|arg| resolver.render(arg, &at))
                    .collect::<Result<_, _>>()?,
                env: env
                    .iter()
                    .map(|(key, value)| Ok((key.clone(), resolver.render(value, &at)?)))
                    .collect::<Result<_, PlanError>>()?,
            }
        }
        OpenWithAction::Reveal { config_file } => {
            let file = manifest.config_files.get(config_file).ok_or_else(|| {
                PlanError::UnrepresentableValue {
                    field: format!("openWith.{}: configFiles.{config_file}", spec.name),
                }
            })?;
            RenderedOpenValue::Reveal {
                path: resolver.path(&file.destination)?,
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_with_names_follow_the_pattern() {
        assert!(is_open_name("psql"));
        assert!(is_open_name("table_plus2"));
        assert!(!is_open_name("2psql"));
        assert!(!is_open_name("Psql"));
        assert!(!is_open_name("table-plus"));
        assert!(!is_open_name(&"a".repeat(33)));
    }

    #[test]
    fn bundle_ids_are_dot_separated_ascii_parts() {
        assert!(is_bundle_id("com.tinyapp.TablePlus"));
        assert!(is_bundle_id("at.eggerapps.Postico2"));
        assert!(is_bundle_id("dev.stackly.fake-client"));
        assert!(!is_bundle_id("TablePlus"));
        assert!(!is_bundle_id("com..app"));
        assert!(!is_bundle_id("com.app."));
        assert!(!is_bundle_id("com.tiny app"));
        assert!(!is_bundle_id("com.app_x"));
        assert!(!is_bundle_id(&format!("com.{}", "a".repeat(255))));
    }

    #[test]
    fn only_http_and_https_count_as_http() {
        assert!(is_http("http://127.0.0.1:1/"));
        assert!(is_http("HTTPS://[::1]:1/"));
        assert!(!is_http("file:///etc"));
        assert!(!is_http("javascript:x"));
    }
}
