//! Controlled interpolation: `${path}` tokens inside manifest strings,
//! typed and checked against the position they appear in (spec 0009,
//! "Interpolasi terkontrol"). There are no functions, defaults,
//! expressions, or nested tokens, and the result is always one string.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};

use serde::Serialize;
use stackly_domain::EndpointName;
use stackly_platform::InstancePaths;

use super::issue::{FileIssues, IssueCode};
use super::names::{COMPONENT_RULE, SettingName, is_component, is_component_byte, is_segment};
use super::settings::{FieldKind, ScalarKind, SettingsSchema};

/// Longest socket name, in bytes, counting every port token as 5 bytes
/// (`SOCKET_NAME_MAX` of spec 0004).
const SOCKET_NAME_MAX: usize = 15;
/// Bytes a port token counts for in a socket name.
const PORT_TOKEN_LEN: usize = 5;
/// The folder under `run` that holds secret files (spec 0011).
pub(crate) const SECRETS_DIR: &str = "secrets";

/// A folder of an instance that the manifest can name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub enum InstanceDir {
    /// The software's data folder.
    Data,
    /// The rendered configuration.
    Config,
    /// The instance's log folder.
    Logs,
    /// Scratch space, also `HOME` and `TMPDIR` of the child.
    Temp,
    /// The short run folder for sockets and PID files.
    Run,
}

impl InstanceDir {
    /// Every folder, in the order plans create them.
    pub const ALL: [Self; 5] = [Self::Data, Self::Config, Self::Logs, Self::Temp, Self::Run];

    /// Returns the manifest code, such as `data`.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::Data => "data",
            Self::Config => "config",
            Self::Logs => "logs",
            Self::Temp => "temp",
            Self::Run => "run",
        }
    }

    /// Parses a manifest code.
    #[must_use]
    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|dir| dir.as_code() == code)
    }

    /// Returns this folder of an instance.
    #[must_use]
    pub fn path(self, paths: &InstancePaths) -> &Path {
        match self {
            Self::Data => &paths.data,
            Self::Config => &paths.config,
            Self::Logs => &paths.logs,
            Self::Temp => &paths.temp,
            Self::Run => &paths.run,
        }
    }
}

/// The transport of a manifest endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub enum EndpointTransport {
    /// TCP on a loopback address.
    Tcp,
    /// UDP on a loopback address.
    Udp,
    /// A Unix socket in the instance's run folder.
    Unix,
}

impl EndpointTransport {
    /// Returns whether the endpoint has an IP host and port.
    #[must_use]
    pub const fn is_ip(self) -> bool {
        matches!(self, Self::Tcp | Self::Udp)
    }
}

/// One resolved token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Token {
    InstanceId,
    InstanceName,
    InstanceShortKey,
    InstanceDir(InstanceDir),
    InstanceSecrets,
    RuntimeRoot,
    RuntimeExecutable,
    RuntimeAlias(String),
    RuntimeVersion,
    RuntimeLine,
    EndpointHost(EndpointName),
    EndpointPort(EndpointName),
    EndpointAddress(EndpointName),
    EndpointSocket(EndpointName),
    EndpointSocketDir(EndpointName),
    Setting(SettingName),
    Secret(SettingName),
    ProviderId,
    ProviderRevision,
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InstanceId => f.write_str("instance.id"),
            Self::InstanceName => f.write_str("instance.name"),
            Self::InstanceShortKey => f.write_str("instance.shortKey"),
            Self::InstanceDir(dir) => write!(f, "instance.{}", dir.as_code()),
            Self::InstanceSecrets => f.write_str("instance.secrets"),
            Self::RuntimeRoot => f.write_str("runtime.root"),
            Self::RuntimeExecutable => f.write_str("runtime.executable"),
            Self::RuntimeAlias(alias) => write!(f, "runtime.executables.{alias}"),
            Self::RuntimeVersion => f.write_str("runtime.version"),
            Self::RuntimeLine => f.write_str("runtime.line"),
            Self::EndpointHost(name) => write!(f, "endpoint.{name}.host"),
            Self::EndpointPort(name) => write!(f, "endpoint.{name}.port"),
            Self::EndpointAddress(name) => write!(f, "endpoint.{name}.address"),
            Self::EndpointSocket(name) => write!(f, "endpoint.{name}.socket"),
            Self::EndpointSocketDir(name) => write!(f, "endpoint.{name}.socketDir"),
            Self::Setting(name) => write!(f, "settings.{name}"),
            Self::Secret(name) => write!(f, "secret.{name}"),
            Self::ProviderId => f.write_str("provider.id"),
            Self::ProviderRevision => f.write_str("provider.revision"),
        }
    }
}

/// The type of a token's value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TokenType {
    Text,
    Integer,
    Boolean,
    Folder,
    RuntimeRoot,
    Executable,
    Secret,
    Array,
}

/// One piece of an interpolated string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Piece {
    Literal(String),
    Token(Token),
}

/// A checked interpolated string.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Interp {
    pub(crate) pieces: Vec<Piece>,
}

impl Interp {
    /// Returns the tokens in order.
    pub(crate) fn tokens(&self) -> impl Iterator<Item = &Token> {
        self.pieces.iter().filter_map(|piece| match piece {
            Piece::Token(token) => Some(token),
            Piece::Literal(_) => None,
        })
    }
}

/// The executable a command runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ExecRef {
    Runtime,
    Alias(String),
}

/// The folder token a path starts with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PathBase {
    /// An instance folder, such as `${instance.config}`.
    Dir(InstanceDir),
    /// `${instance.secrets}`: the `secrets` folder in the run folder.
    Secrets,
}

impl PathBase {
    /// Returns the folder this base names for an instance.
    pub(crate) fn path(self, paths: &InstancePaths) -> PathBuf {
        match self {
            Self::Dir(dir) => dir.path(paths).to_path_buf(),
            Self::Secrets => paths.run.join(SECRETS_DIR),
        }
    }
}

/// A path under one instance folder: the folder token, then literal
/// components.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PathExpr {
    pub(crate) base: PathBase,
    pub(crate) components: Vec<String>,
}

/// Where a path appears, which decides its allowed folders and length.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PathPosition {
    Cwd,
    /// A `configFiles` destination; a sensitive one must start with
    /// `${instance.secrets}` and any other one must not.
    ConfigDestination {
        sensitive: bool,
    },
    LogFile,
}

/// Where a value string appears.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ValuePosition {
    Arg,
    /// An env value; `secrets` is true for the commands that resolve
    /// secrets (`lifecycle.start` and `runExecutable`), where the whole
    /// value may be one `${secret.<field>}` token.
    EnvValue {
        secrets: bool,
    },
    Managed,
}

/// What a token may refer to while a package is validated.
pub(crate) struct TypeEnv<'a> {
    pub(crate) endpoints: &'a BTreeMap<EndpointName, EndpointTransport>,
    pub(crate) aliases: &'a BTreeSet<String>,
    pub(crate) settings: Option<&'a SettingsSchema>,
    pub(crate) managed_fs: &'a BTreeSet<InstanceDir>,
}

/// A parsed piece before its token is resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RawPiece {
    Literal(String),
    Token(Vec<String>),
}

/// Splits text into literals and token paths. `$${` is a literal `${`;
/// any other `$` is literal.
pub(crate) fn parse(text: &str) -> Result<Vec<RawPiece>, String> {
    let mut pieces = Vec::new();
    let mut literal = String::new();
    let mut rest = text;
    while let Some(at) = rest.find('$') {
        literal.push_str(&rest[..at]);
        let after = &rest[at..];
        if let Some(tail) = after.strip_prefix("$${") {
            literal.push_str("${");
            rest = tail;
        } else if let Some(tail) = after.strip_prefix("${") {
            let Some(end) = tail.find('}') else {
                return Err("token is not closed with }".to_owned());
            };
            let path = &tail[..end];
            let segments: Vec<String> = path.split('.').map(str::to_owned).collect();
            if !segments.iter().all(|segment| is_segment(segment)) {
                return Err(format!(
                    "token ${{{path}}} must be dot separated names of letters and digits"
                ));
            }
            if !literal.is_empty() {
                pieces.push(RawPiece::Literal(std::mem::take(&mut literal)));
            }
            pieces.push(RawPiece::Token(segments));
            rest = &tail[end + 1..];
        } else {
            literal.push('$');
            rest = &after[1..];
        }
    }
    literal.push_str(rest);
    if !literal.is_empty() {
        pieces.push(RawPiece::Literal(literal));
    }
    Ok(pieces)
}

/// Why a token was rejected.
pub(crate) type TokenProblem = (IssueCode, String);

/// Resolves a token path against the package and types it.
pub(crate) fn resolve(
    segments: &[String],
    env: &TypeEnv<'_>,
) -> Result<(Token, TokenType), TokenProblem> {
    let parts: Vec<&str> = segments.iter().map(String::as_str).collect();
    let shown = parts.join(".");
    let unknown = || (IssueCode::UnknownToken, format!("unknown token {shown}"));
    let folder = |dir: InstanceDir, token: Token| {
        if env.managed_fs.contains(&dir) {
            Ok((token, TokenType::Folder))
        } else {
            Err((
                IssueCode::PathNotPermitted,
                format!(
                    "{shown} uses the {} folder, which permissions.managedFilesystem does not list",
                    dir.as_code()
                ),
            ))
        }
    };
    match parts.as_slice() {
        ["instance", "id"] => Ok((Token::InstanceId, TokenType::Text)),
        ["instance", "name"] => Ok((Token::InstanceName, TokenType::Text)),
        ["instance", "shortKey"] => Ok((Token::InstanceShortKey, TokenType::Text)),
        ["instance", "secrets"] => {
            if env.managed_fs.contains(&InstanceDir::Run) {
                Ok((Token::InstanceSecrets, TokenType::Folder))
            } else {
                Err((
                    IssueCode::PathNotPermitted,
                    format!(
                        "{shown} lies in the run folder, which permissions.managedFilesystem does not list"
                    ),
                ))
            }
        }
        ["instance", dir] => match InstanceDir::from_code(dir) {
            Some(dir) => folder(dir, Token::InstanceDir(dir)),
            None => Err(unknown()),
        },
        ["runtime", "root"] => Ok((Token::RuntimeRoot, TokenType::RuntimeRoot)),
        ["runtime", "executable"] => Ok((Token::RuntimeExecutable, TokenType::Executable)),
        ["runtime", "executables", alias] => {
            if env.aliases.contains(*alias) {
                Ok((
                    Token::RuntimeAlias((*alias).to_owned()),
                    TokenType::Executable,
                ))
            } else {
                Err((
                    IssueCode::UnknownExecutable,
                    format!("permissions.additionalExecutables has no alias {alias}"),
                ))
            }
        }
        ["runtime", "version"] => Ok((Token::RuntimeVersion, TokenType::Text)),
        ["runtime", "line"] => Ok((Token::RuntimeLine, TokenType::Text)),
        ["endpoint", name, attr] => {
            let endpoint = name
                .parse::<EndpointName>()
                .ok()
                .and_then(|name| env.endpoints.get(&name).map(|transport| (name, *transport)));
            let Some((name, transport)) = endpoint else {
                return Err((
                    IssueCode::UnknownEndpoint,
                    format!("{shown} names an endpoint that does not exist"),
                ));
            };
            let needs = |ip: bool| {
                (
                    IssueCode::TokenNotAllowedHere,
                    format!(
                        "{shown} needs a {} endpoint",
                        if ip { "tcp or udp" } else { "unix" }
                    ),
                )
            };
            match *attr {
                "host" if transport.is_ip() => Ok((Token::EndpointHost(name), TokenType::Text)),
                "port" if transport.is_ip() => Ok((Token::EndpointPort(name), TokenType::Integer)),
                "address" if transport.is_ip() => {
                    Ok((Token::EndpointAddress(name), TokenType::Text))
                }
                "host" | "port" | "address" => Err(needs(true)),
                "socket" if !transport.is_ip() => {
                    folder(InstanceDir::Run, Token::EndpointSocket(name))
                }
                "socketDir" if !transport.is_ip() => {
                    folder(InstanceDir::Run, Token::EndpointSocketDir(name))
                }
                "socket" | "socketDir" => Err(needs(false)),
                _ => Err(unknown()),
            }
        }
        ["settings", field] => {
            let Some(schema) = env.settings else {
                // The schema did not decode; its own issue is reported.
                return Ok((Token::Setting(placeholder_name(field)), TokenType::Text));
            };
            let Some(spec) = schema.field(field) else {
                return Err((
                    IssueCode::UnknownSetting,
                    format!("the settings schema has no field {field}"),
                ));
            };
            if spec.is_secret() {
                return Err((
                    IssueCode::TokenNotAllowedHere,
                    format!("{field} is a secret field, referenced only as secret.{field}"),
                ));
            }
            let kind = match spec.kind() {
                FieldKind::Scalar(ScalarKind::Text(_)) => TokenType::Text,
                FieldKind::Scalar(ScalarKind::Integer(_)) => TokenType::Integer,
                FieldKind::Scalar(ScalarKind::Boolean(_)) => TokenType::Boolean,
                FieldKind::Array(_) => TokenType::Array,
            };
            Ok((Token::Setting(spec.name().clone()), kind))
        }
        ["secret", field] => {
            let Some(schema) = env.settings else {
                return Ok((Token::Secret(placeholder_name(field)), TokenType::Secret));
            };
            match schema.field(field) {
                Some(spec) if spec.is_secret() => {
                    Ok((Token::Secret(spec.name().clone()), TokenType::Secret))
                }
                _ => Err((
                    IssueCode::UnknownSecret,
                    format!("the settings schema has no secret field {field}"),
                )),
            }
        }
        ["provider", "id"] => Ok((Token::ProviderId, TokenType::Text)),
        ["provider", "revision"] => Ok((Token::ProviderRevision, TokenType::Text)),
        _ => Err(unknown()),
    }
}

/// A setting name for a token whose schema could not be read. The name is
/// never used: a package with a broken schema never validates.
fn placeholder_name(field: &str) -> SettingName {
    field.parse().unwrap_or_else(|_| {
        "invalid"
            .parse()
            .expect("`invalid` is a valid setting name")
    })
}

/// Parses and resolves every piece, reporting each problem at `path`.
fn resolve_all(
    text: &str,
    env: &TypeEnv<'_>,
    issues: &mut FileIssues<'_>,
    path: &str,
) -> Option<Vec<(Piece, Option<TokenType>)>> {
    let raw = match parse(text) {
        Ok(raw) => raw,
        Err(message) => {
            issues.at(path, IssueCode::UnknownToken, message);
            return None;
        }
    };
    let mut ok = true;
    let mut pieces = Vec::new();
    for piece in raw {
        match piece {
            RawPiece::Literal(text) => pieces.push((Piece::Literal(text), None)),
            RawPiece::Token(segments) => match resolve(&segments, env) {
                Ok((token, kind)) => pieces.push((Piece::Token(token), Some(kind))),
                Err((code, message)) => {
                    issues.at(path, code, message);
                    ok = false;
                }
            },
        }
    }
    ok.then_some(pieces)
}

/// Checks a value string: an `args` entry, an env value, or a `managed`
/// expression.
pub(crate) fn check_value(
    text: &str,
    position: ValuePosition,
    env: &TypeEnv<'_>,
    issues: &mut FileIssues<'_>,
    path: &str,
) -> Option<Interp> {
    let pieces = resolve_all(text, env, issues, path)?;
    let whole_token = pieces.len() == 1;
    let mut ok = true;
    for (piece, kind) in &pieces {
        let (Piece::Token(token), Some(kind)) = (piece, kind) else {
            continue;
        };
        let problem = match (position, kind) {
            (ValuePosition::EnvValue { secrets: true }, TokenType::Secret) if whole_token => None,
            (ValuePosition::EnvValue { secrets: true }, TokenType::Secret) => Some((
                IssueCode::SecretNotAllowedHere,
                format!("{token} in env must be the whole value, with no other text"),
            )),
            (_, TokenType::Secret) => Some((
                IssueCode::SecretNotAllowedHere,
                format!(
                    "{token} can only be used in stdin, materializeSecret, a sensitive template, or as a whole env value of lifecycle.start or runExecutable"
                ),
            )),
            (_, TokenType::Array) => Some((
                IssueCode::TokenNotAllowedHere,
                format!("{token} is a list, used only through #each in a template"),
            )),
            (ValuePosition::Managed, _) if !managed_allows(token) => Some((
                IssueCode::TokenNotAllowedHere,
                format!("{token} cannot be used in a managed expression"),
            )),
            _ => None,
        };
        if let Some((code, message)) = problem {
            issues.at(path, code, message);
            ok = false;
        }
    }
    if position == ValuePosition::Arg
        && let Some((Piece::Token(Token::Setting(name)), _)) = pieces.first()
        && let Some(field) = env.settings.and_then(|schema| schema.field(name.as_str()))
        && field.is_free_user_value()
    {
        issues.at(
            path,
            IssueCode::ArgumentInjectionRisk,
            format!(
                "an argument cannot start with settings.{name}, whose value could become a flag; put a literal such as --name= first"
            ),
        );
        ok = false;
    }
    ok.then(|| Interp {
        pieces: pieces.into_iter().map(|(piece, _)| piece).collect(),
    })
}

/// Whether a managed expression may use the token: instance, runtime
/// version and line, endpoint, and provider tokens only, and not the
/// secrets folder.
fn managed_allows(token: &Token) -> bool {
    !matches!(
        token,
        Token::Setting(_)
            | Token::Secret(_)
            | Token::InstanceSecrets
            | Token::RuntimeRoot
            | Token::RuntimeExecutable
            | Token::RuntimeAlias(_)
    )
}

/// Checks the `executable` of a command: exactly one executable token.
pub(crate) fn check_executable(
    text: &str,
    env: &TypeEnv<'_>,
    issues: &mut FileIssues<'_>,
    path: &str,
) -> Option<ExecRef> {
    let pieces = resolve_all(text, env, issues, path)?;
    match pieces.as_slice() {
        [(Piece::Token(Token::RuntimeExecutable), _)] => Some(ExecRef::Runtime),
        [(Piece::Token(Token::RuntimeAlias(alias)), _)] => Some(ExecRef::Alias(alias.clone())),
        [(Piece::Token(token), _)] => {
            issues.at(
                path,
                IssueCode::TokenNotAllowedHere,
                format!("{token} is not an executable token"),
            );
            None
        }
        _ => {
            issues.at(
                path,
                IssueCode::LiteralExecutable,
                "executable must be exactly ${runtime.executable} or ${runtime.executables.<alias>}",
            );
            None
        }
    }
}

/// Checks a path position: one instance folder token, then literal
/// components.
pub(crate) fn check_path(
    text: &str,
    position: PathPosition,
    env: &TypeEnv<'_>,
    issues: &mut FileIssues<'_>,
    path: &str,
) -> Option<PathExpr> {
    let pieces = resolve_all(text, env, issues, path)?;
    let mut pieces = pieces.into_iter();
    let base = match pieces.next() {
        Some((Piece::Token(Token::InstanceDir(dir)), _)) => PathBase::Dir(dir),
        Some((Piece::Token(Token::InstanceSecrets), _)) => PathBase::Secrets,
        Some((Piece::Token(token), _)) => {
            issues.at(
                path,
                IssueCode::TokenNotAllowedHere,
                format!("{token} cannot start a path; use an instance folder token"),
            );
            return None;
        }
        _ => {
            issues.at(
                path,
                IssueCode::PathEscapesScope,
                "a path must start with an instance folder token such as ${instance.data}",
            );
            return None;
        }
    };
    let refusal = match (position, base) {
        (PathPosition::LogFile, PathBase::Dir(InstanceDir::Logs))
        | (
            PathPosition::Cwd | PathPosition::ConfigDestination { sensitive: false },
            PathBase::Dir(_),
        )
        | (PathPosition::ConfigDestination { sensitive: true }, PathBase::Secrets) => None,
        (PathPosition::ConfigDestination { sensitive: true }, PathBase::Dir(dir)) => Some(format!(
            "a sensitive config file must be written to ${{instance.secrets}}, not ${{instance.{}}}",
            dir.as_code()
        )),
        (PathPosition::LogFile, PathBase::Dir(dir)) => {
            Some(format!("instance.{} cannot start this path", dir.as_code()))
        }
        (PathPosition::ConfigDestination { sensitive: false }, PathBase::Secrets) => Some(
            "instance.secrets holds only sensitive config files; set sensitive: true".to_owned(),
        ),
        (PathPosition::Cwd | PathPosition::LogFile, PathBase::Secrets) => {
            Some("instance.secrets cannot start this path".to_owned())
        }
    };
    if let Some(message) = refusal {
        issues.at(path, IssueCode::TokenNotAllowedHere, message);
        return None;
    }
    let mut suffix = String::new();
    for (piece, _) in pieces {
        match piece {
            Piece::Literal(text) => suffix.push_str(&text),
            Piece::Token(token) => {
                issues.at(
                    path,
                    IssueCode::TokenNotAllowedHere,
                    format!("{token} cannot appear after the folder token in a path"),
                );
                return None;
            }
        }
    }
    if !suffix.is_empty() && !suffix.starts_with('/') {
        issues.at(
            path,
            IssueCode::InvalidValue,
            "the folder token must be followed by / or nothing",
        );
        return None;
    }
    let mut components = Vec::new();
    for component in suffix.split('/') {
        match component {
            "" | "." => {}
            ".." => {
                issues.at(
                    path,
                    IssueCode::PathEscapesScope,
                    "a path cannot hold a .. component",
                );
                return None;
            }
            _ if is_component(component) => components.push(component.to_owned()),
            _ => {
                issues.at(path, IssueCode::InvalidName, COMPONENT_RULE);
                return None;
            }
        }
    }
    let count_ok = match position {
        PathPosition::Cwd => true,
        PathPosition::ConfigDestination { .. } => (1..=2).contains(&components.len()),
        PathPosition::LogFile => !components.is_empty(),
    };
    if !count_ok {
        let rule = match position {
            PathPosition::ConfigDestination { .. } => {
                "a config destination needs one or two components after the folder token"
            }
            _ => "a log file needs a file name after ${instance.logs}",
        };
        issues.at(path, IssueCode::InvalidValue, rule);
        return None;
    }
    Some(PathExpr { base, components })
}

/// Checks a `materializeSecret` destination: exactly one literal file name,
/// which the executor puts in `${instance.secrets}`. The step needs `run` in
/// `permissions.managedFilesystem`.
pub(crate) fn check_secret_file_name(
    text: &str,
    env: &TypeEnv<'_>,
    issues: &mut FileIssues<'_>,
    path: &str,
) -> Option<String> {
    let literal = match parse(text) {
        Ok(pieces) => match pieces.as_slice() {
            [RawPiece::Literal(literal)] => literal.clone(),
            [] => String::new(),
            _ => {
                issues.at(
                    path,
                    IssueCode::TokenNotAllowedHere,
                    "the destination is a file name only; the file always goes in ${instance.secrets}",
                );
                return None;
            }
        },
        Err(message) => {
            issues.at(path, IssueCode::UnknownToken, message);
            return None;
        }
    };
    if literal.split('/').any(|part| part == "..") {
        issues.at(
            path,
            IssueCode::PathEscapesScope,
            "the destination is a file name in ${instance.secrets}, without ..",
        );
        return None;
    }
    if !is_component(&literal) {
        issues.at(path, IssueCode::InvalidName, COMPONENT_RULE);
        return None;
    }
    if !env.managed_fs.contains(&InstanceDir::Run) {
        issues.at(
            path,
            IssueCode::PathNotPermitted,
            "materializeSecret writes into ${instance.secrets} in the run folder, which permissions.managedFilesystem does not list",
        );
        return None;
    }
    Some(literal)
}

/// Checks a `socketName`: literal socket characters and `${endpoint.<tcp>.port}`
/// tokens, at most 15 bytes with each port counted as 5.
pub(crate) fn check_socket_name(
    text: &str,
    env: &TypeEnv<'_>,
    issues: &mut FileIssues<'_>,
    path: &str,
) -> Option<Interp> {
    let pieces = resolve_all(text, env, issues, path)?;
    let mut len = 0;
    let mut literal = String::new();
    for (piece, _) in &pieces {
        match piece {
            Piece::Literal(text) => {
                if !text.bytes().all(is_component_byte) {
                    issues.at(
                        path,
                        IssueCode::InvalidName,
                        "socket names hold only letters, digits, and . _ -",
                    );
                    return None;
                }
                len += text.len();
                literal.push_str(text);
            }
            Piece::Token(Token::EndpointPort(name))
                if env.endpoints.get(name) == Some(&EndpointTransport::Tcp) =>
            {
                len += PORT_TOKEN_LEN;
            }
            Piece::Token(token) => {
                issues.at(
                    path,
                    IssueCode::TokenNotAllowedHere,
                    format!("{token} cannot appear in a socket name; only tcp endpoint ports can"),
                );
                return None;
            }
        }
    }
    if len == 0 || (literal.len() == len && (literal == "." || literal == "..")) {
        issues.at(
            path,
            IssueCode::InvalidName,
            "a socket name cannot be empty, . or ..",
        );
        return None;
    }
    if len > SOCKET_NAME_MAX {
        issues.at(path, IssueCode::SocketNameTooLong, format!("socket name is {len} bytes with each port counted as 5, more than {SOCKET_NAME_MAX}"));
        return None;
    }
    Some(Interp {
        pieces: pieces.into_iter().map(|(piece, _)| piece).collect(),
    })
}

/// Checks a `connection.template` (spec 0021): text and integer tokens
/// limited to endpoint addresses and ports, the instance name and ID, the
/// provider ID and revision, and the runtime version; the socket path and
/// socket folder of a unix endpoint; and `secret.<field>`. Every other
/// token, `endpoint.<n>.host` and any `settings.*` included, is refused
/// with `token_not_allowed_here`.
pub(crate) fn check_connection_template(
    text: &str,
    env: &TypeEnv<'_>,
    issues: &mut FileIssues<'_>,
    path: &str,
) -> Option<Interp> {
    let raw = match parse(text) {
        Ok(raw) => raw,
        Err(message) => {
            issues.at(path, IssueCode::UnknownToken, message);
            return None;
        }
    };
    let mut ok = true;
    let mut pieces = Vec::new();
    for piece in raw {
        let segments = match piece {
            RawPiece::Literal(text) => {
                pieces.push(Piece::Literal(text));
                continue;
            }
            RawPiece::Token(segments) => segments,
        };
        if segments.first().map(String::as_str) == Some("settings") {
            issues.at(
                path,
                IssueCode::TokenNotAllowedHere,
                format!(
                    "{} cannot be used in a connection template",
                    segments.join(".")
                ),
            );
            ok = false;
            continue;
        }
        match resolve(&segments, env) {
            Ok((token, _)) if connection_allows(&token) => pieces.push(Piece::Token(token)),
            Ok((token, _)) => {
                issues.at(
                    path,
                    IssueCode::TokenNotAllowedHere,
                    format!("{token} cannot be used in a connection template"),
                );
                ok = false;
            }
            Err((code, message)) => {
                issues.at(path, code, message);
                ok = false;
            }
        }
    }
    ok.then_some(Interp { pieces })
}

/// Whether a connection template may use the token (the `connection.template`
/// row of the position matrix, spec 0009).
const fn connection_allows(token: &Token) -> bool {
    matches!(
        token,
        Token::EndpointAddress(_)
            | Token::EndpointPort(_)
            | Token::EndpointSocket(_)
            | Token::EndpointSocketDir(_)
            | Token::InstanceName
            | Token::InstanceId
            | Token::ProviderId
            | Token::ProviderRevision
            | Token::RuntimeVersion
            | Token::Secret(_)
    )
}

/// Checks a literal-only position such as an HTTP probe path, returning
/// the text with `$${` unescaped.
pub(crate) fn check_literal(text: &str, issues: &mut FileIssues<'_>, path: &str) -> Option<String> {
    match parse(text) {
        Ok(pieces) => {
            let mut literal = String::new();
            for piece in pieces {
                match piece {
                    RawPiece::Literal(text) => literal.push_str(&text),
                    RawPiece::Token(_) => {
                        issues.at(
                            path,
                            IssueCode::TokenNotAllowedHere,
                            "this field takes literal text only",
                        );
                        return None;
                    }
                }
            }
            Some(literal)
        }
        Err(message) => {
            issues.at(path, IssueCode::UnknownToken, message);
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use stackly_domain::InstanceId;
    use stackly_platform::DirectoryLayout;
    use stackly_testkit::TestRoot;

    use super::*;
    use crate::provider::issue::{Issue, Issues, Location};

    #[test]
    fn the_secrets_token_names_the_secrets_folder_of_the_layout() {
        let root = TestRoot::new().unwrap();
        let layout = DirectoryLayout::new(root.roots());
        let id = InstanceId::generate();

        assert_eq!(
            PathBase::Secrets.path(&layout.instance_paths(id)),
            layout.instance_secrets_dir(id.short_key())
        );
    }

    fn literal(text: &str) -> RawPiece {
        RawPiece::Literal(text.to_owned())
    }

    fn token(path: &str) -> RawPiece {
        RawPiece::Token(path.split('.').map(str::to_owned).collect())
    }

    #[test]
    fn parse_splits_literals_and_tokens() {
        assert_eq!(
            parse("--config=${instance.config}/nats.conf").unwrap(),
            [
                literal("--config="),
                token("instance.config"),
                literal("/nats.conf")
            ]
        );
        assert_eq!(parse("${a}${b}").unwrap(), [token("a"), token("b")]);
        assert_eq!(parse("").unwrap(), []);
    }

    #[test]
    fn a_doubled_dollar_escapes_a_token_and_other_dollars_stay_literal() {
        assert_eq!(parse("$${HOME}").unwrap(), [literal("${HOME}")]);
        assert_eq!(parse("a$b$").unwrap(), [literal("a$b$")]);
        assert_eq!(parse("$$x").unwrap(), [literal("$$x")]);
    }

    #[test]
    fn unclosed_or_malformed_tokens_are_rejected() {
        assert!(parse("${instance.data").is_err());
        assert!(parse("${}").is_err());
        assert!(parse("${a..b}").is_err());
        assert!(parse("${a.${b}}").is_err());
        assert!(parse("${a-b}").is_err());
    }

    fn env_parts() -> (
        BTreeMap<EndpointName, EndpointTransport>,
        BTreeSet<String>,
        BTreeSet<InstanceDir>,
    ) {
        let endpoints = BTreeMap::from([
            ("client".parse().unwrap(), EndpointTransport::Tcp),
            ("dns".parse().unwrap(), EndpointTransport::Udp),
            ("sock".parse().unwrap(), EndpointTransport::Unix),
        ]);
        let aliases = BTreeSet::from(["cli".to_owned()]);
        let managed = BTreeSet::from([InstanceDir::Data, InstanceDir::Config, InstanceDir::Run]);
        (endpoints, aliases, managed)
    }

    fn check<T>(
        f: impl FnOnce(&TypeEnv<'_>, &mut FileIssues<'_>) -> Option<T>,
    ) -> Result<T, Vec<Issue>> {
        check_with_managed(&env_parts().2, f)
    }

    fn check_without_run<T>(
        f: impl FnOnce(&TypeEnv<'_>, &mut FileIssues<'_>) -> Option<T>,
    ) -> Result<T, Vec<Issue>> {
        check_with_managed(&BTreeSet::from([InstanceDir::Data, InstanceDir::Config]), f)
    }

    fn check_with_managed<T>(
        managed_fs: &BTreeSet<InstanceDir>,
        f: impl FnOnce(&TypeEnv<'_>, &mut FileIssues<'_>) -> Option<T>,
    ) -> Result<T, Vec<Issue>> {
        let (endpoints, aliases, _) = env_parts();
        let env = TypeEnv {
            endpoints: &endpoints,
            aliases: &aliases,
            settings: None,
            managed_fs,
        };
        let mut issues = Issues::new();
        let result = f(&env, &mut FileIssues::new("provider.yaml", &mut issues));
        result.ok_or_else(|| issues.into_vec())
    }

    fn codes(issues: &[Issue]) -> Vec<IssueCode> {
        issues.iter().map(|issue| issue.code).collect()
    }

    #[test]
    fn executables_must_be_exactly_one_executable_token() {
        assert_eq!(
            check(|e, i| check_executable("${runtime.executable}", e, i, "x")),
            Ok(ExecRef::Runtime)
        );
        assert_eq!(
            check(|e, i| check_executable("${runtime.executables.cli}", e, i, "x")),
            Ok(ExecRef::Alias("cli".into()))
        );
        let sh = check(|e, i| check_executable("/bin/sh", e, i, "lifecycle.start.executable"))
            .unwrap_err();
        let flag =
            check(|e, i| check_executable("${runtime.executable} -c", e, i, "x")).unwrap_err();
        let data = check(|e, i| check_executable("${instance.data}", e, i, "x")).unwrap_err();
        let alias =
            check(|e, i| check_executable("${runtime.executables.nope}", e, i, "x")).unwrap_err();

        assert_eq!(codes(&sh), [IssueCode::LiteralExecutable]);
        assert_eq!(
            sh[0].location,
            Location::Field("lifecycle.start.executable".into())
        );
        assert_eq!(codes(&flag), [IssueCode::LiteralExecutable]);
        assert_eq!(codes(&data), [IssueCode::TokenNotAllowedHere]);
        assert_eq!(codes(&alias), [IssueCode::UnknownExecutable]);
    }

    #[test]
    fn folder_tokens_need_their_folder_in_managed_filesystem() {
        let ok =
            check(|e, i| check_value("--data=${instance.data}", ValuePosition::Arg, e, i, "x"));
        let temp =
            check(|e, i| check_value("--data=${instance.temp}", ValuePosition::Arg, e, i, "x"))
                .unwrap_err();

        assert!(ok.is_ok());
        assert_eq!(codes(&temp), [IssueCode::PathNotPermitted]);
    }

    #[test]
    fn endpoint_tokens_check_the_endpoint_and_its_transport() {
        assert!(
            check(|e, i| check_value("${endpoint.client.address}", ValuePosition::Arg, e, i, "x"))
                .is_ok()
        );
        assert!(
            check(|e, i| check_value("${endpoint.dns.port}", ValuePosition::Arg, e, i, "x"))
                .is_ok()
        );
        assert!(
            check(|e, i| check_value("${endpoint.sock.socket}", ValuePosition::Arg, e, i, "x"))
                .is_ok()
        );
        let missing =
            check(|e, i| check_value("${endpoint.web.port}", ValuePosition::Arg, e, i, "x"))
                .unwrap_err();
        let unix_port =
            check(|e, i| check_value("${endpoint.sock.port}", ValuePosition::Arg, e, i, "x"))
                .unwrap_err();
        let tcp_socket =
            check(|e, i| check_value("${endpoint.client.socket}", ValuePosition::Arg, e, i, "x"))
                .unwrap_err();

        assert_eq!(codes(&missing), [IssueCode::UnknownEndpoint]);
        assert_eq!(codes(&unix_port), [IssueCode::TokenNotAllowedHere]);
        assert_eq!(codes(&tcp_socket), [IssueCode::TokenNotAllowedHere]);
    }

    #[test]
    fn unknown_tokens_and_runtime_tokens_in_managed_expressions_are_rejected() {
        let unknown =
            check(|e, i| check_value("${unknown.x}", ValuePosition::Arg, e, i, "x")).unwrap_err();
        let root = check(|e, i| check_value("${runtime.root}", ValuePosition::Managed, e, i, "x"))
            .unwrap_err();

        assert_eq!(codes(&unknown), [IssueCode::UnknownToken]);
        assert_eq!(codes(&root), [IssueCode::TokenNotAllowedHere]);
        assert!(
            check(|e, i| check_value(
                "${instance.config}/dynamic",
                ValuePosition::Managed,
                e,
                i,
                "x"
            ))
            .is_ok()
        );
    }

    #[test]
    fn paths_stay_under_their_folder_token() {
        assert_eq!(
            check(|e, i| check_path(
                "${instance.config}/dynamic//routes.yaml",
                PathPosition::ConfigDestination { sensitive: false },
                e,
                i,
                "x"
            )),
            Ok(PathExpr {
                base: PathBase::Dir(InstanceDir::Config),
                components: vec!["dynamic".into(), "routes.yaml".into()],
            })
        );
        assert_eq!(
            check(|e, i| check_path("${instance.data}/./x", PathPosition::Cwd, e, i, "x"))
                .map(|p| p.components),
            Ok(vec!["x".to_owned()])
        );
        let escape =
            check(|e, i| check_path("${instance.data}/../config", PathPosition::Cwd, e, i, "x"))
                .unwrap_err();
        let token = check(|e, i| {
            check_path(
                "${instance.data}/${instance.config}",
                PathPosition::Cwd,
                e,
                i,
                "x",
            )
        })
        .unwrap_err();
        let literal = check(|e, i| check_path("/tmp/x", PathPosition::Cwd, e, i, "x")).unwrap_err();
        let glued = check(|e, i| check_path("${instance.data}x", PathPosition::Cwd, e, i, "x"))
            .unwrap_err();
        let deep = check(|e, i| {
            check_path(
                "${instance.config}/a/b/c",
                PathPosition::ConfigDestination { sensitive: false },
                e,
                i,
                "x",
            )
        })
        .unwrap_err();
        let log_dir =
            check(|e, i| check_path("${instance.data}/x.log", PathPosition::LogFile, e, i, "x"))
                .unwrap_err();
        let bad_component =
            check(|e, i| check_path("${instance.data}/a b", PathPosition::Cwd, e, i, "x"))
                .unwrap_err();

        assert_eq!(codes(&escape), [IssueCode::PathEscapesScope]);
        assert_eq!(codes(&token), [IssueCode::TokenNotAllowedHere]);
        assert_eq!(codes(&literal), [IssueCode::PathEscapesScope]);
        assert_eq!(codes(&glued), [IssueCode::InvalidValue]);
        assert_eq!(codes(&deep), [IssueCode::InvalidValue]);
        assert_eq!(codes(&log_dir), [IssueCode::TokenNotAllowedHere]);
        assert_eq!(codes(&bad_component), [IssueCode::InvalidName]);
    }

    #[test]
    fn the_secrets_folder_is_accepted_in_values_and_sensitive_destinations_only() {
        for position in [
            ValuePosition::Arg,
            ValuePosition::EnvValue { secrets: false },
            ValuePosition::EnvValue { secrets: true },
        ] {
            assert!(
                check(|e, i| check_value(
                    "--token-file=${instance.secrets}/token",
                    position,
                    e,
                    i,
                    "x"
                ))
                .is_ok(),
                "{position:?}"
            );
        }
        assert_eq!(
            check(|e, i| check_path(
                "${instance.secrets}/auth.conf",
                PathPosition::ConfigDestination { sensitive: true },
                e,
                i,
                "x"
            )),
            Ok(PathExpr {
                base: PathBase::Secrets,
                components: vec!["auth.conf".into()],
            })
        );
        let managed =
            check(|e, i| check_value("${instance.secrets}", ValuePosition::Managed, e, i, "x"))
                .unwrap_err();
        let plain = check(|e, i| {
            check_path(
                "${instance.secrets}/auth.conf",
                PathPosition::ConfigDestination { sensitive: false },
                e,
                i,
                "x",
            )
        })
        .unwrap_err();
        let cwd = check(|e, i| check_path("${instance.secrets}", PathPosition::Cwd, e, i, "x"))
            .unwrap_err();
        let log = check(|e, i| {
            check_path(
                "${instance.secrets}/x.log",
                PathPosition::LogFile,
                e,
                i,
                "x",
            )
        })
        .unwrap_err();

        assert_eq!(codes(&managed), [IssueCode::TokenNotAllowedHere]);
        assert_eq!(codes(&plain), [IssueCode::TokenNotAllowedHere]);
        assert_eq!(codes(&cwd), [IssueCode::TokenNotAllowedHere]);
        assert_eq!(codes(&log), [IssueCode::TokenNotAllowedHere]);
    }

    #[test]
    fn a_sensitive_destination_must_start_with_the_secrets_folder() {
        for dir in ["config", "data", "run"] {
            let text = format!("${{instance.{dir}}}/fake.json");
            let issues = check(|e, i| {
                check_path(
                    &text,
                    PathPosition::ConfigDestination { sensitive: true },
                    e,
                    i,
                    "x",
                )
            })
            .unwrap_err();

            assert_eq!(codes(&issues), [IssueCode::TokenNotAllowedHere], "{dir}");
            assert!(issues[0].message.contains("${instance.secrets}"), "{dir}");
        }
    }

    #[test]
    fn a_sensitive_destination_in_an_unmanaged_folder_reports_path_not_permitted_only() {
        let issues = check(|e, i| {
            check_path(
                "${instance.temp}/fake.json",
                PathPosition::ConfigDestination { sensitive: true },
                e,
                i,
                "x",
            )
        })
        .unwrap_err();

        assert_eq!(codes(&issues), [IssueCode::PathNotPermitted]);
    }

    /// Spec 0009 AC-20: every folder token other than the secrets folder is
    /// refused at the start of a sensitive destination once the folder is
    /// managed, so none of them is a way around the rule.
    #[test]
    fn a_sensitive_destination_in_any_managed_folder_but_secrets_is_refused_with_token_not_allowed_here()
     {
        let all = BTreeSet::from([
            InstanceDir::Config,
            InstanceDir::Data,
            InstanceDir::Run,
            InstanceDir::Temp,
            InstanceDir::Logs,
        ]);
        for dir in ["config", "data", "run", "temp", "logs"] {
            let text = format!("${{instance.{dir}}}/fake.json");
            let issues = check_with_managed(&all, |e, i| {
                check_path(
                    &text,
                    PathPosition::ConfigDestination { sensitive: true },
                    e,
                    i,
                    "x",
                )
            })
            .unwrap_err();

            assert_eq!(codes(&issues), [IssueCode::TokenNotAllowedHere], "{dir}");
            assert!(issues[0].message.contains("${instance.secrets}"), "{dir}");
            assert!(
                issues[0].message.contains(dir),
                "{dir}: {}",
                issues[0].message
            );
        }
    }

    /// Spec 0009 AC-20: a folder missing from `managedFilesystem` keeps its
    /// single `path_not_permitted`, whichever folder it is; the refusal of
    /// the sensitive position never adds a second issue.
    #[test]
    fn path_not_permitted_wins_over_token_not_allowed_here_for_every_unmanaged_folder() {
        let all = [
            InstanceDir::Config,
            InstanceDir::Data,
            InstanceDir::Run,
            InstanceDir::Temp,
            InstanceDir::Logs,
        ];
        for dir in all {
            let managed: BTreeSet<_> = all.into_iter().filter(|other| *other != dir).collect();
            let text = format!("${{instance.{}}}/fake.json", dir.as_code());
            let issues = check_with_managed(&managed, |e, i| {
                check_path(
                    &text,
                    PathPosition::ConfigDestination { sensitive: true },
                    e,
                    i,
                    "x",
                )
            })
            .unwrap_err();

            assert_eq!(codes(&issues), [IssueCode::PathNotPermitted], "{text}");
        }
    }

    /// Spec 0009 AC-20: a sensitive destination in the secrets folder may
    /// name one intermediate folder, and no more.
    #[test]
    fn a_nested_secrets_destination_keeps_its_intermediate_folder_and_stops_at_two_components() {
        let nested = check(|e, i| {
            check_path(
                "${instance.secrets}/conf/server.json",
                PathPosition::ConfigDestination { sensitive: true },
                e,
                i,
                "x",
            )
        });
        let deep = check(|e, i| {
            check_path(
                "${instance.secrets}/a/b/c.json",
                PathPosition::ConfigDestination { sensitive: true },
                e,
                i,
                "x",
            )
        })
        .unwrap_err();
        let escape = check(|e, i| {
            check_path(
                "${instance.secrets}/../fake.json",
                PathPosition::ConfigDestination { sensitive: true },
                e,
                i,
                "x",
            )
        })
        .unwrap_err();

        assert_eq!(
            nested,
            Ok(PathExpr {
                base: PathBase::Secrets,
                components: vec!["conf".into(), "server.json".into()],
            })
        );
        assert_eq!(codes(&deep), [IssueCode::InvalidValue]);
        assert_eq!(codes(&escape), [IssueCode::PathEscapesScope]);
    }

    #[test]
    fn a_plain_destination_may_start_with_any_managed_folder_but_not_the_secrets_folder() {
        for dir in ["config", "data", "run"] {
            let text = format!("${{instance.{dir}}}/app.conf");
            assert!(
                check(|e, i| check_path(
                    &text,
                    PathPosition::ConfigDestination { sensitive: false },
                    e,
                    i,
                    "x"
                ))
                .is_ok(),
                "{dir}"
            );
        }
        let secrets = check(|e, i| {
            check_path(
                "${instance.secrets}/app.conf",
                PathPosition::ConfigDestination { sensitive: false },
                e,
                i,
                "x",
            )
        })
        .unwrap_err();

        assert_eq!(codes(&secrets), [IssueCode::TokenNotAllowedHere]);
    }

    #[test]
    fn the_secrets_folder_needs_run_in_managed_filesystem() {
        let value = check_without_run(|e, i| {
            check_value("${instance.secrets}/token", ValuePosition::Arg, e, i, "x")
        })
        .unwrap_err();
        let destination = check_without_run(|e, i| {
            check_path(
                "${instance.secrets}/auth.conf",
                PathPosition::ConfigDestination { sensitive: true },
                e,
                i,
                "x",
            )
        })
        .unwrap_err();

        assert_eq!(codes(&value), [IssueCode::PathNotPermitted]);
        assert_eq!(codes(&destination), [IssueCode::PathNotPermitted]);
    }

    #[test]
    fn a_secret_env_value_is_one_whole_token_where_secrets_resolve() {
        let whole = check(|e, i| {
            check_value(
                "${secret.authToken}",
                ValuePosition::EnvValue { secrets: true },
                e,
                i,
                "x",
            )
        })
        .unwrap();
        let glued = check(|e, i| {
            check_value(
                "--pass=${secret.authToken}",
                ValuePosition::EnvValue { secrets: true },
                e,
                i,
                "x",
            )
        })
        .unwrap_err();
        let two = check(|e, i| {
            check_value(
                "${secret.authToken}${secret.authToken}",
                ValuePosition::EnvValue { secrets: true },
                e,
                i,
                "x",
            )
        })
        .unwrap_err();
        let probe = check(|e, i| {
            check_value(
                "${secret.authToken}",
                ValuePosition::EnvValue { secrets: false },
                e,
                i,
                "x",
            )
        })
        .unwrap_err();
        let arg = check(|e, i| check_value("${secret.authToken}", ValuePosition::Arg, e, i, "x"))
            .unwrap_err();

        assert!(matches!(
            whole.pieces.as_slice(),
            [Piece::Token(Token::Secret(_))]
        ));
        assert_eq!(codes(&glued), [IssueCode::SecretNotAllowedHere]);
        assert_eq!(codes(&two), [IssueCode::SecretNotAllowedHere; 2]);
        assert_eq!(codes(&probe), [IssueCode::SecretNotAllowedHere]);
        assert_eq!(codes(&arg), [IssueCode::SecretNotAllowedHere]);
    }

    #[test]
    fn a_secret_file_name_is_one_literal_component() {
        assert_eq!(
            check(|e, i| check_secret_file_name("token", e, i, "x")),
            Ok("token".to_owned())
        );
        let run =
            check(|e, i| check_secret_file_name("${instance.run}/token", e, i, "x")).unwrap_err();
        let temp =
            check(|e, i| check_secret_file_name("${instance.temp}/token", e, i, "x")).unwrap_err();
        let secrets = check(|e, i| check_secret_file_name("${instance.secrets}/token", e, i, "x"))
            .unwrap_err();
        let nested = check(|e, i| check_secret_file_name("a/token", e, i, "x")).unwrap_err();
        let up = check(|e, i| check_secret_file_name("../token", e, i, "x")).unwrap_err();
        let dot = check(|e, i| check_secret_file_name(".", e, i, "x")).unwrap_err();
        let empty = check(|e, i| check_secret_file_name("", e, i, "x")).unwrap_err();
        let no_run =
            check_without_run(|e, i| check_secret_file_name("token", e, i, "x")).unwrap_err();

        assert_eq!(codes(&run), [IssueCode::TokenNotAllowedHere]);
        assert_eq!(codes(&temp), [IssueCode::TokenNotAllowedHere]);
        assert_eq!(codes(&secrets), [IssueCode::TokenNotAllowedHere]);
        assert_eq!(codes(&nested), [IssueCode::InvalidName]);
        assert_eq!(codes(&up), [IssueCode::PathEscapesScope]);
        assert_eq!(codes(&dot), [IssueCode::InvalidName]);
        assert_eq!(codes(&empty), [IssueCode::InvalidName]);
        assert_eq!(codes(&no_run), [IssueCode::PathNotPermitted]);
    }

    #[test]
    fn socket_names_count_ports_as_five_bytes() {
        assert!(
            check(|e, i| check_socket_name(".s.PGSQL.${endpoint.client.port}", e, i, "x")).is_ok()
        );
        let long = check(|e, i| check_socket_name(".s.PGSQL.${endpoint.client.port}xy", e, i, "x"))
            .unwrap_err();
        let escape = check(|e, i| check_socket_name("../x", e, i, "x")).unwrap_err();
        let dots = check(|e, i| check_socket_name("..", e, i, "x")).unwrap_err();
        let udp = check(|e, i| check_socket_name("s${endpoint.dns.port}", e, i, "x")).unwrap_err();

        assert_eq!(codes(&long), [IssueCode::SocketNameTooLong]);
        assert_eq!(codes(&escape), [IssueCode::InvalidName]);
        assert_eq!(codes(&dots), [IssueCode::InvalidName]);
        assert_eq!(codes(&udp), [IssueCode::TokenNotAllowedHere]);
    }

    #[test]
    fn literal_positions_reject_tokens_and_unescape_doubled_dollars() {
        assert_eq!(
            check(|_, i| check_literal("/healthz", i, "x")),
            Ok("/healthz".to_owned())
        );
        assert_eq!(
            check(|_, i| check_literal("/$${x}", i, "x")),
            Ok("/${x}".to_owned())
        );
        let token = check(|_, i| check_literal("/${instance.id}", i, "x")).unwrap_err();
        assert_eq!(codes(&token), [IssueCode::TokenNotAllowedHere]);
    }
}
