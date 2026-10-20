//! The logic-less config template renderer (spec 0009, "Template
//! config"): `{{ path }}`, `{{#each path as item}}`, `{{#if path}}` with
//! `{{else}}`, `{{! comment }}`, and `\{{` for a literal `{{`. Every output
//! goes through the escaper of the config file's format; there is no raw
//! mode.

use serde::Serialize;

use super::interpolate::{Token, TokenType, TypeEnv, resolve};
use super::issue::{FileIssues, IssueCode, Location};
use super::names::{RESERVED_LOOP_NAMES, SettingName, is_name, is_segment};
use super::plan::{PlanError, RenderedContent, Resolver, Segment, Value};
use super::settings::{FieldKind, ScalarKind, ScalarValue, SettingsItem};

/// Most nested blocks.
const MAX_DEPTH: usize = 4;
/// Largest rendered file, in bytes.
pub const OUTPUT_MAX_LEN: usize = 1024 * 1024;

/// The format of a config file, which picks the escaper.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub enum TemplateFormat {
    /// YAML; text becomes a double quoted scalar.
    Yaml,
    /// JSON; text becomes a JSON string.
    Json,
    /// The NATS server config; text becomes a double quoted string, and
    /// text holding `"`, `\`, `$`, or a control character is rejected.
    NatsConf,
    /// The PostgreSQL server config (spec 0019): text becomes a single
    /// quoted value with `'` doubled and `\` doubled, text holding a
    /// control character is rejected, and a boolean is `on` or `off`.
    PostgresqlConf,
    /// A static file (spec 0019): validation rejects every output and
    /// block tag, so the template comes out as written.
    Plain,
}

impl TemplateFormat {
    /// Parses a manifest code.
    pub(crate) fn from_code(code: &str) -> Option<Self> {
        match code {
            "yaml" => Some(Self::Yaml),
            "json" => Some(Self::Json),
            "natsConf" => Some(Self::NatsConf),
            "postgresqlConf" => Some(Self::PostgresqlConf),
            "plain" => Some(Self::Plain),
            _ => None,
        }
    }

    /// Escapes text, or returns `None` when the format cannot hold it.
    pub(crate) fn escape_text(self, text: &str) -> Option<String> {
        match self {
            Self::Yaml => {
                let mut out = String::with_capacity(text.len() + 2);
                out.push('"');
                for c in text.chars() {
                    match c {
                        '\\' => out.push_str("\\\\"),
                        '"' => out.push_str("\\\""),
                        c if c.is_control()
                            || matches!(c, '\u{2028}' | '\u{2029}' | '\u{feff}') =>
                        {
                            out.push_str(&format!("\\u{:04X}", u32::from(c)));
                        }
                        c => out.push(c),
                    }
                }
                out.push('"');
                Some(out)
            }
            Self::Json => Some(serde_json::Value::String(text.to_owned()).to_string()),
            Self::NatsConf => {
                if text
                    .chars()
                    .any(|c| matches!(c, '"' | '\\' | '$') || c.is_control())
                {
                    None
                } else {
                    Some(format!("\"{text}\""))
                }
            }
            Self::PostgresqlConf => {
                if text.chars().any(char::is_control) {
                    return None;
                }
                let mut out = String::with_capacity(text.len() + 2);
                out.push('\'');
                for c in text.chars() {
                    match c {
                        '\'' => out.push_str("''"),
                        '\\' => out.push_str("\\\\"),
                        c => out.push(c),
                    }
                }
                out.push('\'');
                Some(out)
            }
            // Validation rejects every output tag of a plain template.
            Self::Plain => None,
        }
    }

    /// Writes a boolean the way this format reads it.
    pub(crate) fn boolean(self, flag: bool) -> &'static str {
        match (self, flag) {
            (Self::PostgresqlConf, true) => "on",
            (Self::PostgresqlConf, false) => "off",
            (_, true) => "true",
            (_, false) => "false",
        }
    }

    /// Returns whether templates of this format may hold output and block
    /// tags; a `plain` template holds only text and comments.
    pub(crate) fn allows_tags(self) -> bool {
        !matches!(self, Self::Plain)
    }

    /// Checks that rendered output is a valid document of this format.
    pub(crate) fn output_parses(self, text: &str) -> bool {
        match self {
            Self::Yaml => super::raw::yaml_value(text).is_ok(),
            Self::Json => serde_json::from_str::<serde_json::Value>(text).is_ok(),
            Self::NatsConf | Self::PostgresqlConf | Self::Plain => true,
        }
    }
}

/// A line and column, both starting at 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Pos {
    pub(crate) line: usize,
    pub(crate) column: usize,
}

impl Pos {
    fn location(self) -> Location {
        Location::Position {
            line: self.line,
            column: self.column,
        }
    }
}

/// A parsed template, before its paths are resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Template {
    nodes: Vec<Node<Vec<String>>>,
}

/// A template whose paths resolved against one package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Checked {
    nodes: Vec<Node<Ref>>,
    /// Every token the template uses, with where.
    pub(crate) tokens: Vec<(Token, Pos)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Node<P> {
    Text(String),
    Output {
        path: P,
        at: Pos,
    },
    Each {
        path: P,
        var: String,
        body: Vec<Node<P>>,
        at: Pos,
    },
    If {
        path: P,
        then: Vec<Node<P>>,
        otherwise: Vec<Node<P>>,
        at: Pos,
    },
}

/// A resolved template path.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Ref {
    Token(Token),
    /// A field of the item of the loop `depth` levels up (0 = innermost).
    LoopField {
        depth: usize,
        var: String,
        field: SettingName,
    },
    LoopFirst(usize),
    LoopLast(usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TagKind {
    Output(Vec<String>),
    EachOpen(Vec<String>, String),
    EachClose,
    IfOpen(Vec<String>),
    Else,
    IfClose,
    Comment,
}

impl TagKind {
    const fn is_block(&self) -> bool {
        !matches!(self, Self::Output(_))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Lex {
    Text(String),
    Tag(TagKind, Pos),
}

/// A template syntax error.
pub(crate) type SyntaxError = (Pos, String);

/// Parses a template.
pub(crate) fn parse(text: &str) -> Result<Template, SyntaxError> {
    let mut lexed = lex(text)?;
    strip_standalone(&mut lexed);
    build(lexed)
}

fn lex(text: &str) -> Result<Vec<Lex>, SyntaxError> {
    let mut out = Vec::new();
    let mut literal = String::new();
    let mut pos = Pos { line: 1, column: 1 };
    let mut rest = text;
    let advance = |pos: &mut Pos, consumed: &str| {
        for c in consumed.chars() {
            if c == '\n' {
                pos.line += 1;
                pos.column = 1;
            } else {
                pos.column += 1;
            }
        }
    };
    while !rest.is_empty() {
        let next = rest.find("{{").map(|at| {
            if at > 0 && rest.as_bytes()[at - 1] == b'\\' {
                (at - 1, true)
            } else {
                (at, false)
            }
        });
        let Some((at, escaped)) = next else {
            literal.push_str(rest);
            break;
        };
        literal.push_str(&rest[..at]);
        advance(&mut pos, &rest[..at]);
        if escaped {
            literal.push_str("{{");
            advance(&mut pos, "\\{{");
            rest = &rest[at + 3..];
            continue;
        }
        let start = pos;
        let body = &rest[at + 2..];
        let Some(end) = body.find("}}") else {
            return Err((start, "tag is not closed with }}".to_owned()));
        };
        let kind = classify(body[..end].trim()).map_err(|message| (start, message))?;
        if !literal.is_empty() {
            out.push(Lex::Text(std::mem::take(&mut literal)));
        }
        out.push(Lex::Tag(kind, start));
        advance(&mut pos, &rest[at..at + 2 + end + 2]);
        rest = &body[end + 2..];
    }
    if !literal.is_empty() {
        out.push(Lex::Text(literal));
    }
    Ok(out)
}

fn classify(content: &str) -> Result<TagKind, String> {
    let path = |text: &str| -> Result<Vec<String>, String> {
        let segments: Vec<String> = text.split('.').map(str::to_owned).collect();
        if segments.iter().all(|segment| is_segment(segment)) {
            Ok(segments)
        } else {
            Err(format!(
                "{text} is not a path of letter and digit names separated by dots"
            ))
        }
    };
    if content.starts_with('!') {
        return Ok(TagKind::Comment);
    }
    match content {
        "/each" => return Ok(TagKind::EachClose),
        "/if" => return Ok(TagKind::IfClose),
        "else" => return Ok(TagKind::Else),
        _ => {}
    }
    if let Some(rest) = content.strip_prefix("#each ") {
        let words: Vec<&str> = rest.split_whitespace().collect();
        return match words.as_slice() {
            [list, "as", var] => Ok(TagKind::EachOpen(path(list)?, (*var).to_owned())),
            _ => Err("each takes the form {{#each path as name}}".to_owned()),
        };
    }
    if let Some(rest) = content.strip_prefix("#if ") {
        return Ok(TagKind::IfOpen(path(rest.trim())?));
    }
    if content.starts_with('#') || content.starts_with('/') {
        return Err(format!("unknown block tag {content}"));
    }
    Ok(TagKind::Output(path(content)?))
}

fn is_blank(text: &str) -> bool {
    text.chars().all(|c| matches!(c, ' ' | '\t' | '\r'))
}

/// Removes every line that holds only whitespace and one block tag,
/// newline included.
fn strip_standalone(lexed: &mut [Lex]) {
    let last = lexed.len().saturating_sub(1);
    let mut cuts: Vec<(usize, bool)> = Vec::new();
    for (index, item) in lexed.iter().enumerate() {
        let Lex::Tag(kind, _) = item else {
            continue;
        };
        if !kind.is_block() {
            continue;
        }
        let before_ok = match index.checked_sub(1).map(|at| &lexed[at]) {
            None => true,
            Some(Lex::Text(text)) => match text.rfind('\n') {
                Some(nl) => is_blank(&text[nl + 1..]),
                None => index == 1 && is_blank(text),
            },
            Some(Lex::Tag(..)) => false,
        };
        let after_ok = match lexed.get(index + 1) {
            None => true,
            Some(Lex::Text(text)) => match text.find('\n') {
                Some(nl) => is_blank(&text[..nl]),
                None => index + 1 == last && is_blank(text),
            },
            Some(Lex::Tag(..)) => false,
        };
        if before_ok && after_ok {
            if index > 0 {
                cuts.push((index - 1, true));
            }
            if index < last {
                cuts.push((index + 1, false));
            }
        }
    }
    // Decide every cut on the original text, then apply them.
    let mut plans: Vec<(Option<usize>, Option<usize>)> = vec![(None, None); lexed.len()];
    for (at, trailing) in cuts {
        let Lex::Text(text) = &lexed[at] else {
            continue;
        };
        if trailing {
            plans[at].1 = Some(text.rfind('\n').map_or(0, |nl| nl + 1));
        } else {
            plans[at].0 = Some(text.find('\n').map_or(text.len(), |nl| nl + 1));
        }
    }
    for (item, (lead, trail)) in lexed.iter_mut().zip(plans) {
        if let Lex::Text(text) = item {
            let start = lead.unwrap_or(0);
            let end = trail.unwrap_or(text.len()).max(start);
            *text = text[start..end].to_owned();
        }
    }
}

fn build(lexed: Vec<Lex>) -> Result<Template, SyntaxError> {
    enum Frame {
        Each {
            path: Vec<String>,
            var: String,
            at: Pos,
            body: Vec<Node<Vec<String>>>,
        },
        If {
            path: Vec<String>,
            at: Pos,
            then: Vec<Node<Vec<String>>>,
            otherwise: Option<Vec<Node<Vec<String>>>>,
        },
    }
    fn current<'a>(
        root: &'a mut Vec<Node<Vec<String>>>,
        stack: &'a mut [Frame],
    ) -> &'a mut Vec<Node<Vec<String>>> {
        match stack.last_mut() {
            None => root,
            Some(Frame::Each { body, .. }) => body,
            Some(Frame::If {
                then, otherwise, ..
            }) => otherwise.as_mut().unwrap_or(then),
        }
    }
    let mut root = Vec::new();
    let mut stack: Vec<Frame> = Vec::new();
    for item in lexed {
        match item {
            Lex::Text(text) => {
                if !text.is_empty() {
                    current(&mut root, &mut stack).push(Node::Text(text));
                }
            }
            Lex::Tag(kind, at) => match kind {
                TagKind::Comment => {}
                TagKind::Output(path) => {
                    current(&mut root, &mut stack).push(Node::Output { path, at })
                }
                TagKind::EachOpen(..) | TagKind::IfOpen(_) if stack.len() == MAX_DEPTH => {
                    return Err((at, format!("blocks nest more than {MAX_DEPTH} deep")));
                }
                TagKind::EachOpen(path, var) => stack.push(Frame::Each {
                    path,
                    var,
                    at,
                    body: Vec::new(),
                }),
                TagKind::IfOpen(path) => stack.push(Frame::If {
                    path,
                    at,
                    then: Vec::new(),
                    otherwise: None,
                }),
                TagKind::Else => match stack.last_mut() {
                    Some(Frame::If {
                        otherwise: otherwise @ None,
                        ..
                    }) => *otherwise = Some(Vec::new()),
                    _ => return Err((at, "else appears outside an if block or twice".to_owned())),
                },
                TagKind::EachClose => match stack.pop() {
                    Some(Frame::Each {
                        path,
                        var,
                        at,
                        body,
                    }) => {
                        current(&mut root, &mut stack).push(Node::Each {
                            path,
                            var,
                            body,
                            at,
                        });
                    }
                    _ => return Err((at, "/each does not close an each block".to_owned())),
                },
                TagKind::IfClose => match stack.pop() {
                    Some(Frame::If {
                        path,
                        at,
                        then,
                        otherwise,
                    }) => {
                        let otherwise = otherwise.unwrap_or_default();
                        current(&mut root, &mut stack).push(Node::If {
                            path,
                            then,
                            otherwise,
                            at,
                        });
                    }
                    _ => return Err((at, "/if does not close an if block".to_owned())),
                },
            },
        }
    }
    match stack.last() {
        None => Ok(Template { nodes: root }),
        Some(Frame::Each { at, .. } | Frame::If { at, .. }) => {
            Err((*at, "block is not closed".to_owned()))
        }
    }
}

/// A loop variable in scope while checking.
struct Scope<'a> {
    var: String,
    items: &'a std::collections::BTreeMap<SettingName, super::settings::ItemField>,
}

/// Resolves every path of a template against the package and checks its
/// types, reporting problems with their line and column. Secret use in a
/// template that is not sensitive is checked per config file through
/// [`Checked::secrets`].
pub(crate) fn check(
    template: &Template,
    env: &TypeEnv<'_>,
    issues: &mut FileIssues<'_>,
) -> Option<Checked> {
    let start = issues.count();
    let mut tokens = Vec::new();
    let mut scopes = Vec::new();
    let nodes = check_nodes(&template.nodes, env, &mut scopes, &mut tokens, issues);
    (issues.count() == start).then_some(Checked { nodes, tokens })
}

impl Checked {
    /// Returns where each top level output or block tag starts, for the
    /// check that a `plain` template holds none.
    pub(crate) fn tag_positions(&self) -> impl Iterator<Item = Pos> {
        self.nodes.iter().filter_map(|node| match node {
            Node::Text(_) => None,
            Node::Output { at, .. } | Node::Each { at, .. } | Node::If { at, .. } => Some(*at),
        })
    }

    /// Returns where the template outputs a secret.
    pub(crate) fn secrets(&self) -> impl Iterator<Item = (&SettingName, Pos)> {
        self.tokens.iter().filter_map(|(token, at)| match token {
            Token::Secret(name) => Some((name, *at)),
            _ => None,
        })
    }
}

/// The type of a resolved template path.
enum PathType {
    Scalar,
    Boolean,
    Array,
    Secret,
    Executable,
    Object,
}

fn check_path<'a>(
    path: &[String],
    at: Pos,
    env: &TypeEnv<'a>,
    scopes: &[Scope<'a>],
    tokens: &mut Vec<(Token, Pos)>,
    issues: &mut FileIssues<'_>,
) -> Option<(Ref, PathType)> {
    let shown = path.join(".");
    let first = path.first().map(String::as_str).unwrap_or_default();
    if let Some((depth, scope)) = scopes
        .iter()
        .rev()
        .enumerate()
        .find(|(_, scope)| scope.var == first)
    {
        return match path {
            [_, field] => match scope.items.iter().find(|(name, _)| name.as_str() == field) {
                Some((name, item)) => {
                    let kind = match item.kind {
                        ScalarKind::Boolean(_) => PathType::Boolean,
                        _ => PathType::Scalar,
                    };
                    Some((
                        Ref::LoopField {
                            depth,
                            var: scope.var.clone(),
                            field: name.clone(),
                        },
                        kind,
                    ))
                }
                None => {
                    issues.push(
                        at.location(),
                        IssueCode::UnknownToken,
                        format!("items have no field {field}"),
                    );
                    None
                }
            },
            [_] => Some((
                Ref::LoopField {
                    depth,
                    var: scope.var.clone(),
                    field: "invalid"
                        .parse()
                        .expect("`invalid` is a valid setting name"),
                },
                PathType::Object,
            )),
            _ => {
                issues.push(
                    at.location(),
                    IssueCode::UnknownToken,
                    format!("unknown token {shown}"),
                );
                None
            }
        };
    }
    if first == "loop" {
        return match (path, scopes.is_empty()) {
            ([_, which], false) if which == "first" => Some((Ref::LoopFirst(0), PathType::Boolean)),
            ([_, which], false) if which == "last" => Some((Ref::LoopLast(0), PathType::Boolean)),
            _ => {
                issues.push(
                    at.location(),
                    IssueCode::UnknownToken,
                    format!("{shown} is only loop.first or loop.last inside #each"),
                );
                None
            }
        };
    }
    match resolve(path, env) {
        Ok((token, kind)) => {
            tokens.push((token.clone(), at));
            let kind = match kind {
                TokenType::Boolean => PathType::Boolean,
                TokenType::Array => PathType::Array,
                TokenType::Secret => PathType::Secret,
                TokenType::Executable => PathType::Executable,
                TokenType::Text
                | TokenType::Integer
                | TokenType::Folder
                | TokenType::RuntimeRoot => PathType::Scalar,
            };
            Some((Ref::Token(token), kind))
        }
        Err((code, message)) => {
            issues.push(at.location(), code, message);
            None
        }
    }
}

fn check_nodes<'a>(
    nodes: &[Node<Vec<String>>],
    env: &TypeEnv<'a>,
    scopes: &mut Vec<Scope<'a>>,
    tokens: &mut Vec<(Token, Pos)>,
    issues: &mut FileIssues<'_>,
) -> Vec<Node<Ref>> {
    let mut out = Vec::new();
    for node in nodes {
        match node {
            Node::Text(text) => out.push(Node::Text(text.clone())),
            Node::Output { path, at } => {
                let Some((reference, kind)) = check_path(path, *at, env, scopes, tokens, issues)
                else {
                    continue;
                };
                match kind {
                    PathType::Scalar | PathType::Boolean | PathType::Secret => {
                        out.push(Node::Output {
                            path: reference,
                            at: *at,
                        });
                    }
                    PathType::Executable => issues.push(
                        at.location(),
                        IssueCode::TokenNotAllowedHere,
                        format!(
                            "{} is an executable and cannot be written into a config file",
                            path.join(".")
                        ),
                    ),
                    PathType::Array | PathType::Object => issues.push(
                        at.location(),
                        IssueCode::TemplateType,
                        format!(
                            "{} is not a scalar; use #each to write a list",
                            path.join(".")
                        ),
                    ),
                }
            }
            Node::Each {
                path,
                var,
                body,
                at,
            } => {
                let Some((reference, kind)) = check_path(path, *at, env, scopes, tokens, issues)
                else {
                    continue;
                };
                let items = match (&reference, kind) {
                    (Ref::Token(Token::Setting(name)), PathType::Array) => env
                        .settings
                        .and_then(|schema| schema.field(name.as_str()))
                        .and_then(|field| match field.kind() {
                            FieldKind::Array(rules) => Some(&rules.items),
                            FieldKind::Scalar(_) => None,
                        }),
                    _ => None,
                };
                let Some(items) = items else {
                    issues.push(
                        at.location(),
                        IssueCode::TemplateType,
                        format!(
                            "#each needs a list setting, and {} is not one",
                            path.join(".")
                        ),
                    );
                    continue;
                };
                if !is_name(var)
                    || RESERVED_LOOP_NAMES.contains(&var.as_str())
                    || scopes.iter().any(|scope| &scope.var == var)
                {
                    issues.push(
                        at.location(),
                        IssueCode::InvalidName,
                        format!("loop variable {var} must be a name that is not a namespace or an outer loop variable"),
                    );
                    continue;
                }
                scopes.push(Scope {
                    var: var.clone(),
                    items,
                });
                let body = check_nodes(body, env, scopes, tokens, issues);
                scopes.pop();
                out.push(Node::Each {
                    path: reference,
                    var: var.clone(),
                    body,
                    at: *at,
                });
            }
            Node::If {
                path,
                then,
                otherwise,
                at,
            } => {
                let Some((reference, kind)) = check_path(path, *at, env, scopes, tokens, issues)
                else {
                    continue;
                };
                if !matches!(kind, PathType::Boolean | PathType::Array) {
                    issues.push(
                        at.location(),
                        IssueCode::TemplateType,
                        format!(
                            "#if needs a boolean or a list, and {} is neither",
                            path.join(".")
                        ),
                    );
                    continue;
                }
                let then = check_nodes(then, env, scopes, tokens, issues);
                let otherwise = check_nodes(otherwise, env, scopes, tokens, issues);
                out.push(Node::If {
                    path: reference,
                    then,
                    otherwise,
                    at: *at,
                });
            }
        }
    }
    out
}

/// One loop being rendered.
struct Frame<'a> {
    item: &'a SettingsItem,
    index: usize,
    len: usize,
}

/// Rendered output being built.
struct Output {
    segments: Vec<Segment>,
    text: String,
    len: usize,
    format: TemplateFormat,
    config_file: String,
}

impl Output {
    fn push_text(&mut self, text: &str) -> Result<(), PlanError> {
        self.len += text.len();
        if self.len > OUTPUT_MAX_LEN {
            return Err(PlanError::OutputTooLarge {
                config_file: self.config_file.clone(),
            });
        }
        self.text.push_str(text);
        Ok(())
    }

    fn push_value(&mut self, value: Value, field: &str) -> Result<(), PlanError> {
        match value {
            Value::Text(text) => {
                let escaped = self.format.escape_text(&text).ok_or_else(|| {
                    PlanError::UnrepresentableValue {
                        field: field.to_owned(),
                    }
                })?;
                self.push_text(&escaped)
            }
            Value::Integer(number) => self.push_text(&number.to_string()),
            Value::Boolean(flag) => self.push_text(self.format.boolean(flag)),
            Value::Secret(secret) => {
                if !self.text.is_empty() {
                    self.segments
                        .push(Segment::Text(std::mem::take(&mut self.text)));
                }
                self.segments.push(Segment::Secret {
                    secret,
                    format: self.format,
                });
                Ok(())
            }
        }
    }
}

/// Renders a checked template with the values of one plan.
pub(crate) fn render(
    checked: &Checked,
    format: TemplateFormat,
    resolver: &Resolver<'_>,
    config_file: &str,
) -> Result<RenderedContent, PlanError> {
    let mut output = Output {
        segments: Vec::new(),
        text: String::new(),
        len: 0,
        format,
        config_file: config_file.to_owned(),
    };
    let mut frames = Vec::new();
    render_nodes(&checked.nodes, resolver, &mut frames, &mut output)?;
    if !output.text.is_empty() {
        output.segments.push(Segment::Text(output.text));
    }
    Ok(RenderedContent {
        config_file: config_file.to_owned(),
        segments: output.segments,
    })
}

fn lookup(
    reference: &Ref,
    resolver: &Resolver<'_>,
    frames: &[Frame<'_>],
) -> Result<Value, PlanError> {
    let frame = |depth: usize| {
        frames
            .len()
            .checked_sub(depth + 1)
            .and_then(|at| frames.get(at))
    };
    match reference {
        Ref::Token(token) => resolver.value(token),
        Ref::LoopField { depth, field, .. } => {
            match frame(*depth).and_then(|frame| frame.item.get(field.as_str())) {
                Some(ScalarValue::Text(text)) => Ok(Value::Text(text.clone())),
                Some(ScalarValue::Integer(number)) => Ok(Value::Integer(*number)),
                Some(ScalarValue::Boolean(flag)) => Ok(Value::Boolean(*flag)),
                None => Err(PlanError::Settings(super::settings::SettingsError {
                    issues: vec![super::issue::Issue::new(
                        super::settings::SETTINGS_FILE,
                        Location::Field(format!("{}[].{field}", field_name(reference))),
                        IssueCode::SettingsValue,
                        "item field is missing",
                    )],
                })),
            }
        }
        Ref::LoopFirst(depth) => Ok(Value::Boolean(
            frame(*depth).is_some_and(|frame| frame.index == 0),
        )),
        Ref::LoopLast(depth) => Ok(Value::Boolean(
            frame(*depth).is_some_and(|frame| frame.index + 1 == frame.len),
        )),
    }
}

fn field_name(reference: &Ref) -> String {
    match reference {
        Ref::Token(token) => token.to_string(),
        Ref::LoopField { var, field, .. } => format!("{var}.{field}"),
        Ref::LoopFirst(_) => "loop.first".to_owned(),
        Ref::LoopLast(_) => "loop.last".to_owned(),
    }
}

fn render_nodes<'a>(
    nodes: &[Node<Ref>],
    resolver: &'a Resolver<'_>,
    frames: &mut Vec<Frame<'a>>,
    output: &mut Output,
) -> Result<(), PlanError> {
    for node in nodes {
        match node {
            Node::Text(text) => output.push_text(text)?,
            Node::Output { path, .. } => {
                let value = lookup(path, resolver, frames)?;
                output.push_value(value, &field_name(path))?;
            }
            Node::Each { path, body, .. } => {
                let Ref::Token(Token::Setting(name)) = path else {
                    continue;
                };
                let items = resolver.items(name);
                for (index, item) in items.iter().enumerate() {
                    frames.push(Frame {
                        item,
                        index,
                        len: items.len(),
                    });
                    let result = render_nodes(body, resolver, frames, output);
                    frames.pop();
                    result?;
                }
            }
            Node::If {
                path,
                then,
                otherwise,
                ..
            } => {
                let truthy = match path {
                    Ref::Token(Token::Setting(name)) if resolver.is_array(name) => {
                        !resolver.items(name).is_empty()
                    }
                    _ => matches!(lookup(path, resolver, frames)?, Value::Boolean(true)),
                };
                render_nodes(
                    if truthy { then } else { otherwise },
                    resolver,
                    frames,
                    output,
                )?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(template: &Template) -> Vec<String> {
        fn walk(nodes: &[Node<Vec<String>>], out: &mut Vec<String>) {
            for node in nodes {
                match node {
                    Node::Text(text) => out.push(text.clone()),
                    Node::Output { path, .. } => out.push(format!("<{}>", path.join("."))),
                    Node::Each { body, .. } => {
                        out.push("[each".into());
                        walk(body, out);
                        out.push("]".into());
                    }
                    Node::If {
                        then, otherwise, ..
                    } => {
                        out.push("[if".into());
                        walk(then, out);
                        out.push("|".into());
                        walk(otherwise, out);
                        out.push("]".into());
                    }
                }
            }
        }
        let mut out = Vec::new();
        walk(&template.nodes, &mut out);
        out
    }

    #[test]
    fn lines_with_only_a_block_tag_disappear_with_their_newline() {
        let template =
            parse("a: 1\n{{#if settings.x}}\n  b: {{ settings.y }}\n{{/if}}\nc: 2\n").unwrap();

        assert_eq!(
            texts(&template),
            [
                "a: 1\n",
                "[if",
                "  b: ",
                "<settings.y>",
                "\n",
                "|",
                "]",
                "c: 2\n"
            ]
        );
    }

    #[test]
    fn indented_block_tags_and_comments_also_disappear() {
        let template =
            parse("  {{! note }}\nx\n    {{#each settings.r as r}}\n- {{ r.n }}\n    {{/each}}\n")
                .unwrap();

        assert_eq!(texts(&template), ["x\n", "[each", "- ", "<r.n>", "\n", "]"]);
    }

    #[test]
    fn inline_block_tags_stay_in_place() {
        let template = parse("a {{#if settings.x}}b{{/if}} c\n").unwrap();

        assert_eq!(texts(&template), ["a ", "[if", "b", "|", "]", " c\n"]);
    }

    #[test]
    fn an_escaped_brace_pair_is_literal() {
        let template = parse("\\{{ x }}").unwrap();

        assert_eq!(texts(&template), ["{{ x }}"]);
    }

    #[test]
    fn syntax_errors_carry_their_line_and_column() {
        let unclosed = parse("ok\n  {{ settings.x").unwrap_err();
        let unbalanced = parse("{{#if settings.x}}\n").unwrap_err();
        let stray = parse("{{/each}}").unwrap_err();
        let nested = parse(
            "{{#if a}}{{#if a}}{{#if a}}{{#if a}}{{#if a}}{{/if}}{{/if}}{{/if}}{{/if}}{{/if}}",
        )
        .unwrap_err();
        let bad_path = parse("{{ a..b }}").unwrap_err();
        let unknown_block = parse("{{#with a}}{{/with}}").unwrap_err();

        assert_eq!(unclosed.0, Pos { line: 2, column: 3 });
        assert_eq!(unbalanced.0, Pos { line: 1, column: 1 });
        assert_eq!(stray.0, Pos { line: 1, column: 1 });
        assert_eq!(
            nested.0,
            Pos {
                line: 1,
                column: 37
            }
        );
        assert!(nested.1.contains("more than 4"), "{}", nested.1);
        assert_eq!(bad_path.0, Pos { line: 1, column: 1 });
        assert!(
            unknown_block.1.contains("unknown block"),
            "{}",
            unknown_block.1
        );
        assert!(parse("{{#if a}}{{#if a}}{{#if a}}{{#if a}}x{{/if}}{{/if}}{{/if}}{{/if}}").is_ok());
    }

    #[test]
    fn yaml_escaping_quotes_and_escapes_control_and_separator_characters() {
        assert_eq!(
            TemplateFormat::Yaml.escape_text("a\"b\\c").unwrap(),
            "\"a\\\"b\\\\c\""
        );
        assert_eq!(
            TemplateFormat::Yaml
                .escape_text("x\n\u{7f}\u{85}\u{2028}\u{2029}\u{feff}")
                .unwrap(),
            "\"x\\u000A\\u007F\\u0085\\u2028\\u2029\\uFEFF\""
        );
        assert_eq!(
            TemplateFormat::Yaml.escape_text("ü 空").unwrap(),
            "\"ü 空\""
        );
    }

    #[test]
    fn yaml_escaped_text_parses_back_to_the_same_text() {
        let nasty = "$(touch pwned); rm -rf ~ `id` \"q\" 'q' | & > ü 空\t\u{1}: # [x]";
        let escaped = TemplateFormat::Yaml.escape_text(nasty).unwrap();

        let back: String = serde_yaml_ng::from_str(&format!("v: {escaped}"))
            .map(|value: serde_yaml_ng::Value| value["v"].as_str().unwrap().to_owned())
            .unwrap();

        assert_eq!(back, nasty);
    }

    #[test]
    fn json_escaping_uses_json_strings() {
        assert_eq!(
            TemplateFormat::Json.escape_text("a\"\n").unwrap(),
            "\"a\\\"\\n\""
        );
    }

    #[test]
    fn nats_conf_rejects_quotes_backslashes_dollars_and_control_characters() {
        assert_eq!(
            TemplateFormat::NatsConf.escape_text("orders db").unwrap(),
            "\"orders db\""
        );
        for bad in ["a\"b", "a\\b", "$HOME", "a\nb", "a\u{0}"] {
            assert_eq!(TemplateFormat::NatsConf.escape_text(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn postgresql_conf_single_quotes_text_and_doubles_quotes_and_backslashes() {
        let format = TemplateFormat::PostgresqlConf;

        assert_eq!(format.escape_text("none").unwrap(), "'none'");
        assert_eq!(format.escape_text("a'b\\c d").unwrap(), "'a''b\\\\c d'");
        assert_eq!(format.escape_text("ü 空").unwrap(), "'ü 空'");
        assert_eq!(format.escape_text("").unwrap(), "''");
        assert_eq!(
            format.escape_text("$(touch pwned) \"q\" `id`").unwrap(),
            "'$(touch pwned) \"q\" `id`'"
        );
    }

    #[test]
    fn postgresql_conf_rejects_every_control_character() {
        for bad in [
            "a\nb", "a\tb", "a\rb", "a\u{0}", "a\u{1f}", "a\u{7f}", "a\u{85}", "a\u{9f}",
        ] {
            assert_eq!(
                TemplateFormat::PostgresqlConf.escape_text(bad),
                None,
                "{bad:?}"
            );
        }
        assert!(
            TemplateFormat::PostgresqlConf
                .escape_text("a\u{a0}b")
                .is_some()
        );
    }

    #[test]
    fn postgresql_conf_writes_booleans_as_on_and_off_and_other_formats_as_true_and_false() {
        assert_eq!(TemplateFormat::PostgresqlConf.boolean(true), "on");
        assert_eq!(TemplateFormat::PostgresqlConf.boolean(false), "off");
        for format in [
            TemplateFormat::Yaml,
            TemplateFormat::Json,
            TemplateFormat::NatsConf,
        ] {
            assert_eq!(format.boolean(true), "true");
            assert_eq!(format.boolean(false), "false");
        }
    }

    #[test]
    fn plain_escapes_nothing_and_allows_no_tags() {
        assert_eq!(TemplateFormat::Plain.escape_text("x"), None);
        assert!(!TemplateFormat::Plain.allows_tags());
        assert!(TemplateFormat::PostgresqlConf.allows_tags());
        assert!(TemplateFormat::Plain.output_parses("local all all scram-sha-256\n"));
    }

    #[test]
    fn the_new_formats_parse_from_their_manifest_codes() {
        assert_eq!(
            TemplateFormat::from_code("postgresqlConf"),
            Some(TemplateFormat::PostgresqlConf)
        );
        assert_eq!(
            TemplateFormat::from_code("plain"),
            Some(TemplateFormat::Plain)
        );
        assert_eq!(TemplateFormat::from_code("postgresql"), None);
        assert_eq!(TemplateFormat::from_code("Plain"), None);
    }
}
