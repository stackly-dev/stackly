//! Controlled interpolation: `${path}` tokens inside manifest strings,
//! typed and checked against the position they appear in (spec 0009,
//! "Interpolasi terkontrol"). There are no functions, defaults,
//! expressions, or nested tokens, and the result is always one string.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::Path;

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

/// A path under one instance folder: the folder token, then literal
/// components.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PathExpr {
    pub(crate) base: InstanceDir,
    pub(crate) components: Vec<String>,
}

/// Where a path appears, which decides its allowed folders and length.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PathPosition {
    Cwd,
    ConfigDestination,
    SecretDestination,
    LogFile,
}

/// Where a value string appears.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ValuePosition {
    Arg,
    EnvValue,
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
    let mut ok = true;
    for (piece, kind) in &pieces {
        let (Piece::Token(token), Some(kind)) = (piece, kind) else {
            continue;
        };
        let problem = match (position, kind) {
            (_, TokenType::Secret) => Some((
                IssueCode::SecretNotAllowedHere,
                format!(
                    "{token} can only be used in stdin, materializeSecret, or a sensitive template"
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
/// version and line, endpoint, and provider tokens only.
fn managed_allows(token: &Token) -> bool {
    !matches!(
        token,
        Token::Setting(_)
            | Token::Secret(_)
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
        Some((Piece::Token(Token::InstanceDir(dir)), _)) => dir,
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
    let allowed: &[InstanceDir] = match position {
        PathPosition::Cwd | PathPosition::ConfigDestination => &InstanceDir::ALL,
        PathPosition::SecretDestination => &[InstanceDir::Run, InstanceDir::Temp],
        PathPosition::LogFile => &[InstanceDir::Logs],
    };
    if !allowed.contains(&base) {
        issues.at(
            path,
            IssueCode::TokenNotAllowedHere,
            format!("instance.{} cannot start this path", base.as_code()),
        );
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
        PathPosition::ConfigDestination => (1..=2).contains(&components.len()),
        PathPosition::SecretDestination => components.len() == 1,
        PathPosition::LogFile => !components.is_empty(),
    };
    if !count_ok {
        let rule = match position {
            PathPosition::ConfigDestination => {
                "a config destination needs one or two components after the folder token"
            }
            PathPosition::SecretDestination => {
                "a secret destination needs exactly one file name after the folder token"
            }
            _ => "a log file needs a file name after ${instance.logs}",
        };
        issues.at(path, IssueCode::InvalidValue, rule);
        return None;
    }
    Some(PathExpr { base, components })
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
    use super::*;
    use crate::provider::issue::{Issue, Issues, Location};

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
        let (endpoints, aliases, managed_fs) = env_parts();
        let env = TypeEnv {
            endpoints: &endpoints,
            aliases: &aliases,
            settings: None,
            managed_fs: &managed_fs,
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
                PathPosition::ConfigDestination,
                e,
                i,
                "x"
            )),
            Ok(PathExpr {
                base: InstanceDir::Config,
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
                PathPosition::ConfigDestination,
                e,
                i,
                "x",
            )
        })
        .unwrap_err();
        let secret_dir = check(|e, i| {
            check_path(
                "${instance.data}/token",
                PathPosition::SecretDestination,
                e,
                i,
                "x",
            )
        })
        .unwrap_err();
        let bad_component =
            check(|e, i| check_path("${instance.data}/a b", PathPosition::Cwd, e, i, "x"))
                .unwrap_err();

        assert_eq!(codes(&escape), [IssueCode::PathEscapesScope]);
        assert_eq!(codes(&token), [IssueCode::TokenNotAllowedHere]);
        assert_eq!(codes(&literal), [IssueCode::PathEscapesScope]);
        assert_eq!(codes(&glued), [IssueCode::InvalidValue]);
        assert_eq!(codes(&deep), [IssueCode::InvalidValue]);
        assert_eq!(codes(&secret_dir), [IssueCode::TokenNotAllowedHere]);
        assert_eq!(codes(&bad_component), [IssueCode::InvalidName]);
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
