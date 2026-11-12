//! The logic-less config template renderer (spec 0009, "Template
//! config"): `{{ path }}`, `{{#each path as item}}`, `{{#if path}}` with
//! `{{else}}`, `{{! comment }}`, and `\{{` for a literal `{{`. Every output
//! goes through the escaper of the config file's format; there is no raw
//! mode, and the one filter, `{{ secret.<field> | sha256Hex }}` (spec
//! 0040 AC-3), writes a hash of a secret that holds only `[0-9a-f]`.

use serde::Serialize;

use super::interpolate::{Token, TokenType, TypeEnv, resolve};
use super::issue::{FileIssues, IssueCode, Location};
use super::names::{RESERVED_LOOP_NAMES, SettingName, is_name, is_segment};
use super::plan::{PlanError, RenderedContent, Resolver, Segment, Value};
use super::settings::{FieldKind, ScalarKind, ScalarValue, SettingsItem};
use crate::secret::delivery::HashAlgo;

/// Most nested blocks.
const MAX_DEPTH: usize = 4;
/// Largest rendered file, in bytes.
pub const OUTPUT_MAX_LEN: usize = 1024 * 1024;

/// Returns whether `c` is in the YAML 1.2 printable set (`c-printable`):
/// tab, line feed, carriage return, `#x20` to `#x7E`, `#x85`, `#xA0` to
/// `#xD7FF`, `#xE000` to `#xFFFD`, and `#x10000` to `#x10FFFF`. A Rust `char`
/// is never a surrogate, so the gap at `#xD800` to `#xDFFF` needs no arm.
fn is_yaml_printable(c: char) -> bool {
    matches!(
        u32::from(c),
        0x9 | 0xA | 0xD
            | 0x20..=0x7E
            | 0x85
            | 0xA0..=0xD7FF
            | 0xE000..=0xFFFD
            | 0x1_0000..=0x10_FFFF
    )
}

/// Writes `c` as a YAML double quoted escape: `\uXXXX` inside the basic
/// multilingual plane, `\UXXXXXXXX` above it.
fn push_yaml_escape(out: &mut String, c: char) {
    let code = u32::from(c);
    if code > 0xFFFF {
        out.push_str(&format!("\\U{code:08X}"));
    } else {
        out.push_str(&format!("\\u{code:04X}"));
    }
}

/// Writes `text` as a double quoted value with `\` as `\\` and `"` as
/// `\"`, leaving every other character as it is, or returns `None` when the
/// text holds a control character (U+0000 to U+001F, U+007F to U+009F).
fn backslash_quoted(text: &str) -> Option<String> {
    if text.chars().any(char::is_control) {
        return None;
    }
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            c => out.push(c),
        }
    }
    out.push('"');
    Some(out)
}

/// Writes `text` as XML content with `&`, `<`, `>`, `"`, and `'` as
/// entities and no quotes around it, or returns `None` when the text holds
/// a control character (U+0000 to U+001F, U+007F to U+009F), U+FFFE, or
/// U+FFFF, which an XML document cannot carry as is.
fn xml_content(text: &str) -> Option<String> {
    if text
        .chars()
        .any(|c| c.is_control() || matches!(c, '\u{fffe}' | '\u{ffff}'))
    {
        return None;
    }
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c => out.push(c),
        }
    }
    Some(out)
}

/// The format of a config file, which picks the escaper.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub enum TemplateFormat {
    /// YAML; text becomes a double quoted scalar.
    Yaml,
    /// JSON; text becomes a JSON string.
    Json,
    /// The NATS server config; text becomes a double quoted string with `"`
    /// and `\` escaped and `$` kept as is, and text holding a control
    /// character is rejected.
    NatsConf,
    /// The PostgreSQL server config (spec 0019): text becomes a single
    /// quoted value with `'` doubled and `\` doubled, text holding a
    /// control character is rejected, and a boolean is `on` or `off`.
    PostgresqlConf,
    /// A static file (spec 0019): validation rejects every output and
    /// block tag, so the template comes out as written.
    Plain,
    /// The Valkey server config (spec 0040): text becomes a double quoted
    /// value with `\` and `"` escaped by a backslash, text holding a
    /// control character is rejected, and a boolean is `yes` or `no`.
    ValkeyConf,
    /// The MySQL option file (spec 0042): text becomes a double quoted
    /// value with `\` and `"` escaped by a backslash, text holding a
    /// control character is rejected, and a boolean is `ON` or `OFF`.
    MysqlCnf,
    /// XML (spec 0043): text becomes element or attribute content with no
    /// quotes around it and `&`, `<`, `>`, `"`, and `'` as entities, so a
    /// tag may touch literal text; text holding a control character,
    /// U+FFFE, or U+FFFF is rejected.
    Xml,
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
            "valkeyConf" => Some(Self::ValkeyConf),
            "mysqlCnf" => Some(Self::MysqlCnf),
            "xml" => Some(Self::Xml),
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
                        // Braces are escaped too: a consumer such as Traefik's
                        // file provider runs the raw file through Go
                        // text/template before it parses the YAML, and must
                        // never see a `{{` delimiter a value brought in.
                        // Control characters (tab, line feed, and U+0085
                        // included) and every character outside the YAML 1.2
                        // printable set, such as U+FFFE and U+FFFF, are
                        // escaped, so the rendered file is always a valid
                        // YAML stream.
                        c if c.is_control()
                            || !is_yaml_printable(c)
                            || matches!(c, '\u{2028}' | '\u{2029}' | '\u{feff}' | '{' | '}') =>
                        {
                            push_yaml_escape(&mut out, c);
                        }
                        c => out.push(c),
                    }
                }
                out.push('"');
                Some(out)
            }
            // The output is one JSON string literal, so every brace in it
            // belongs to the value; escaping them keeps template delimiters
            // out of the raw text, as for YAML.
            Self::Json => Some(
                serde_json::Value::String(text.to_owned())
                    .to_string()
                    .replace('{', "\\u007B")
                    .replace('}', "\\u007D"),
            ),
            // `\"` and `\\` are the escapes of a double quoted NATS string,
            // and `$` inside quotes is literal (spec 0039, fact 4); a control
            // character has no escape the server reads back, so it is
            // rejected.
            Self::NatsConf => {
                if text.chars().any(char::is_control) {
                    return None;
                }
                let mut out = String::with_capacity(text.len() + 2);
                out.push('"');
                for c in text.chars() {
                    match c {
                        '"' => out.push_str("\\\""),
                        '\\' => out.push_str("\\\\"),
                        c => out.push(c),
                    }
                }
                out.push('"');
                Some(out)
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
            Self::ValkeyConf | Self::MysqlCnf => backslash_quoted(text),
            Self::Xml => xml_content(text),
            // Validation rejects every output tag of a plain template.
            Self::Plain => None,
        }
    }

    /// Writes a boolean the way this format reads it.
    pub(crate) fn boolean(self, flag: bool) -> &'static str {
        match (self, flag) {
            (Self::PostgresqlConf, true) => "on",
            (Self::PostgresqlConf, false) => "off",
            (Self::ValkeyConf, true) => "yes",
            (Self::ValkeyConf, false) => "no",
            (Self::MysqlCnf, true) => "ON",
            (Self::MysqlCnf, false) => "OFF",
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
            // These formats are not parsed back; tests against the real
            // binaries in their provider specs guard the escapers.
            Self::NatsConf
            | Self::PostgresqlConf
            | Self::Plain
            | Self::ValkeyConf
            | Self::MysqlCnf
            | Self::Xml => true,
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
    nodes: Vec<ParsedNode>,
}

/// A template whose paths resolved against one package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Checked {
    nodes: Vec<CheckedNode>,
    /// Every token the template uses, with where.
    pub(crate) tokens: Vec<(Token, Pos)>,
}

/// A template node: `P` is the path and `F` the filter of an output tag,
/// as parsed (names) or as checked (the hash it writes).
#[derive(Debug, Clone, PartialEq, Eq)]
enum Node<P, F> {
    Text(String),
    Output {
        path: P,
        filter: F,
        at: Pos,
    },
    Each {
        path: P,
        var: String,
        body: Vec<Node<P, F>>,
        at: Pos,
    },
    If {
        path: P,
        then: Vec<Node<P, F>>,
        otherwise: Vec<Node<P, F>>,
        at: Pos,
    },
}

/// A node as parsed: dotted path segments and the filter names after `|`.
type ParsedNode = Node<Vec<String>, Vec<String>>;

/// A node whose paths resolved; an output tag with the `sha256Hex` filter
/// carries the hash it writes in place of the secret.
type CheckedNode = Node<Ref, Option<HashAlgo>>;

/// The one filter name today (spec 0040 AC-3).
const SHA256_HEX_FILTER: &str = "sha256Hex";

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
    /// A path and the filter names after it.
    Output(Vec<String>, Vec<String>),
    EachOpen(Vec<String>, String),
    EachClose,
    IfOpen(Vec<String>),
    Else,
    IfClose,
    Comment,
}

impl TagKind {
    const fn is_block(&self) -> bool {
        !matches!(self, Self::Output(..))
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
    let mut parts = content.split('|').map(str::trim);
    let target = path(parts.next().unwrap_or_default())?;
    let filters = parts
        .map(|name| {
            if !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric()) {
                Ok(name.to_owned())
            } else {
                Err(format!(
                    "filter {name:?} is not a name of letters and digits after |"
                ))
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(TagKind::Output(target, filters))
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
            body: Vec<ParsedNode>,
        },
        If {
            path: Vec<String>,
            at: Pos,
            then: Vec<ParsedNode>,
            otherwise: Option<Vec<ParsedNode>>,
        },
    }
    fn current<'a>(
        root: &'a mut Vec<ParsedNode>,
        stack: &'a mut [Frame],
    ) -> &'a mut Vec<ParsedNode> {
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
                TagKind::Output(path, filter) => {
                    current(&mut root, &mut stack).push(Node::Output { path, filter, at })
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

/// Checks the filters of an output tag: at most one, only `sha256Hex`, and
/// only after a `secret.*` path. Returns the hash it writes (`None` for a
/// tag without a filter), or `None` with an issue when a rule fails.
#[expect(
    clippy::option_option,
    reason = "the outer None is a rejected tag, the inner None a tag without a filter"
)]
fn check_filter(
    path: &[String],
    filters: &[String],
    kind: &PathType,
    at: Pos,
    issues: &mut FileIssues<'_>,
) -> Option<Option<HashAlgo>> {
    let message = match filters {
        [] => return Some(None),
        [name] if name != SHA256_HEX_FILTER => {
            format!("unknown filter {name}; the only filter is {SHA256_HEX_FILTER}")
        }
        [_] if !matches!(kind, PathType::Secret) => format!(
            "{SHA256_HEX_FILTER} only follows a secret.* path, and {} is not one",
            path.join(".")
        ),
        [_] => return Some(Some(HashAlgo::Sha256Hex)),
        _ => "an output tag takes at most one filter".to_owned(),
    };
    issues.push(at.location(), IssueCode::TemplateType, message);
    None
}

fn check_nodes<'a>(
    nodes: &[ParsedNode],
    env: &TypeEnv<'a>,
    scopes: &mut Vec<Scope<'a>>,
    tokens: &mut Vec<(Token, Pos)>,
    issues: &mut FileIssues<'_>,
) -> Vec<CheckedNode> {
    let mut out = Vec::new();
    for node in nodes {
        match node {
            Node::Text(text) => out.push(Node::Text(text.clone())),
            Node::Output { path, filter, at } => {
                let Some((reference, kind)) = check_path(path, *at, env, scopes, tokens, issues)
                else {
                    continue;
                };
                let Some(filter) = check_filter(path, filter, &kind, *at, issues) else {
                    continue;
                };
                match kind {
                    PathType::Scalar | PathType::Boolean | PathType::Secret => {
                        out.push(Node::Output {
                            path: reference,
                            filter,
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

    /// Ends the current text segment, if any, before a secret segment.
    fn flush_text(&mut self) {
        if !self.text.is_empty() {
            self.segments
                .push(Segment::Text(std::mem::take(&mut self.text)));
        }
    }

    /// Writes the hash of a secret value, which `materialize` computes; the
    /// static check lets a filter follow only a `secret.*` path.
    fn push_hash(&mut self, value: Value, algo: HashAlgo, field: &str) -> Result<(), PlanError> {
        let Value::Secret(secret) = value else {
            return Err(PlanError::UnrepresentableValue {
                field: field.to_owned(),
            });
        };
        self.flush_text();
        self.segments.push(Segment::SecretHash { secret, algo });
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
                self.flush_text();
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
    nodes: &[CheckedNode],
    resolver: &'a Resolver<'_>,
    frames: &mut Vec<Frame<'a>>,
    output: &mut Output,
) -> Result<(), PlanError> {
    for node in nodes {
        match node {
            Node::Text(text) => output.push_text(text)?,
            Node::Output { path, filter, .. } => {
                let value = lookup(path, resolver, frames)?;
                match filter {
                    Some(algo) => output.push_hash(value, *algo, &field_name(path))?,
                    None => output.push_value(value, &field_name(path))?,
                }
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
        fn walk(nodes: &[ParsedNode], out: &mut Vec<String>) {
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

    /// The filter names of the only output tag of a parsed template.
    fn filters(text: &str) -> Vec<String> {
        match parse(text).unwrap().nodes.as_slice() {
            [Node::Output { filter, .. }] => filter.clone(),
            other => panic!("one output tag expected, got {other:?}"),
        }
    }

    // covers: AC-3 (spec 0040)
    #[test]
    fn an_output_tag_takes_filter_names_after_a_bar_with_any_spacing() {
        assert_eq!(filters("{{ secret.x | sha256Hex }}"), ["sha256Hex"]);
        assert_eq!(filters("{{secret.x|sha256Hex}}"), ["sha256Hex"]);
        assert_eq!(filters("{{  secret.x   |\tsha256Hex  }}"), ["sha256Hex"]);
        assert_eq!(filters("{{ secret.x | a | b }}"), ["a", "b"]);
        assert!(filters("{{ secret.x }}").is_empty());
    }

    // covers: AC-3 (spec 0040)
    #[test]
    fn an_empty_or_odd_filter_name_is_a_syntax_error_at_the_tag() {
        for bad in [
            "ok\n {{ secret.x | }}",
            "ok\n {{ secret.x || sha256Hex }}",
            "ok\n {{ secret.x | sha-256 }}",
            "ok\n {{ secret.x | sha256_hex }}",
            "ok\n {{ secret.x | sha 256 }}",
            "ok\n {{ secret.x | ü }}",
            "ok\n {{ | sha256Hex }}",
        ] {
            let (at, message) = parse(bad).unwrap_err();
            assert_eq!(at, Pos { line: 2, column: 2 }, "{bad:?}");
            assert!(!message.is_empty());
        }
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
    fn yaml_escaping_escapes_every_character_outside_the_yaml_printable_set() {
        assert_eq!(
            TemplateFormat::Yaml
                .escape_text("a\u{fffe}b\u{ffff}c\u{fffd}\u{1f600}")
                .unwrap(),
            "\"a\\uFFFEb\\uFFFFc\u{fffd}\u{1f600}\""
        );
        for code in (0..=0x10_FFFF_u32).filter(|code| !(0xD800..=0xDFFF).contains(code)) {
            let c = char::from_u32(code).expect("every non surrogate code point is a char");
            let escaped = TemplateFormat::Yaml.escape_text(&c.to_string()).unwrap();
            assert!(
                escaped.chars().all(is_yaml_printable),
                "U+{code:04X} left a non printable character: {escaped:?}"
            );
        }
    }

    #[test]
    fn yaml_escaped_noncharacters_parse_back_to_the_same_text() {
        let nasty = "rule \u{fffe}\u{ffff}\u{1}\u{9f}\u{85}\u{feff}\u{10ffff}\u{1fffe} {{ x }}";
        let escaped = TemplateFormat::Yaml.escape_text(nasty).unwrap();
        assert!(!escaped.contains('\u{fffe}') && !escaped.contains('\u{ffff}'));

        let back: String = serde_yaml_ng::from_str(&format!("v: {escaped}"))
            .map(|value: serde_yaml_ng::Value| value["v"].as_str().unwrap().to_owned())
            .unwrap();

        assert_eq!(back, nasty);
        assert!(super::super::raw::yaml_value(&format!("v: {escaped}\n")).is_ok());
    }

    #[test]
    fn yaml_and_json_escaping_leave_no_template_delimiter_in_the_raw_text() {
        let rule = "Host(`a.localhost`) && Path(`/{{ printf `%c` 65 }}`) {{";
        for format in [TemplateFormat::Yaml, TemplateFormat::Json] {
            let escaped = format.escape_text(rule).unwrap();
            assert!(
                !escaped.contains('{') && !escaped.contains('}'),
                "{format:?}: {escaped}"
            );
        }

        let yaml = TemplateFormat::Yaml.escape_text(rule).unwrap();
        let back: String = serde_yaml_ng::from_str(&format!("v: {yaml}"))
            .map(|value: serde_yaml_ng::Value| value["v"].as_str().unwrap().to_owned())
            .unwrap();
        assert_eq!(back, rule);
        let json = TemplateFormat::Json.escape_text(rule).unwrap();
        assert_eq!(serde_json::from_str::<String>(&json).unwrap(), rule);
    }

    #[test]
    fn json_escaping_uses_json_strings() {
        assert_eq!(
            TemplateFormat::Json.escape_text("a\"\n").unwrap(),
            "\"a\\\"\\n\""
        );
    }

    #[test]
    fn nats_conf_escapes_a_double_quote_as_backslash_quote() {
        assert_eq!(
            TemplateFormat::NatsConf.escape_text("a\"b").unwrap(),
            "\"a\\\"b\""
        );
    }

    #[test]
    fn nats_conf_escapes_a_backslash_as_two_backslashes() {
        assert_eq!(
            TemplateFormat::NatsConf.escape_text("a\\b").unwrap(),
            "\"a\\\\b\""
        );
    }

    #[test]
    fn nats_conf_writes_a_dollar_sign_as_is() {
        assert_eq!(
            TemplateFormat::NatsConf.escape_text("$HOME").unwrap(),
            "\"$HOME\""
        );
        assert_eq!(
            TemplateFormat::NatsConf.escape_text("a$b$").unwrap(),
            "\"a$b$\""
        );
    }

    #[test]
    fn nats_conf_escapes_quote_backslash_and_dollar_together() {
        assert_eq!(
            TemplateFormat::NatsConf
                .escape_text("p\"q\\r$s-fixture")
                .unwrap(),
            "\"p\\\"q\\\\r$s-fixture\""
        );
    }

    #[test]
    fn nats_conf_rejects_every_control_character() {
        for bad in [
            "a\nb", "a\tb", "a\rb", "a\u{0}", "a\u{1f}", "a\u{7f}", "a\u{85}", "a\u{9f}",
        ] {
            assert_eq!(TemplateFormat::NatsConf.escape_text(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn nats_conf_keeps_other_printable_characters_as_is() {
        assert_eq!(
            TemplateFormat::NatsConf.escape_text("orders db").unwrap(),
            "\"orders db\""
        );
        assert_eq!(
            TemplateFormat::NatsConf
                .escape_text("a #b {c} 'd' ü 空 \u{a0}")
                .unwrap(),
            "\"a #b {c} 'd' ü 空 \u{a0}\""
        );
        assert_eq!(TemplateFormat::NatsConf.escape_text("").unwrap(), "\"\"");
    }

    // covers: AC-2 (spec 0039)
    #[test]
    fn nats_conf_never_lets_a_trailing_backslash_escape_the_closing_quote() {
        assert_eq!(
            TemplateFormat::NatsConf.escape_text("a\\").unwrap(),
            "\"a\\\\\""
        );
        assert_eq!(
            TemplateFormat::NatsConf.escape_text("\\").unwrap(),
            "\"\\\\\""
        );
    }

    // covers: AC-2 (spec 0039)
    #[test]
    fn nats_conf_escapes_an_escaped_quote_in_the_value_as_two_separate_escapes() {
        // The value `\"` is a backslash then a quote: each gets its own
        // escape, so the server reads the same two characters back.
        assert_eq!(
            TemplateFormat::NatsConf.escape_text("\\\"").unwrap(),
            "\"\\\\\\\"\""
        );
        assert_eq!(
            TemplateFormat::NatsConf.escape_text("\"\"\\\\").unwrap(),
            "\"\\\"\\\"\\\\\\\\\""
        );
    }

    // covers: AC-2 (spec 0039)
    #[test]
    fn nats_conf_keeps_every_dollar_form_literal_inside_the_quotes() {
        for text in ["$", "$$", "${VAR}", "$(id)", "$2a$10$abc", "x$"] {
            assert_eq!(
                TemplateFormat::NatsConf.escape_text(text).unwrap(),
                format!("\"{text}\""),
                "{text:?}"
            );
        }
    }

    // covers: AC-2 (spec 0039)
    #[test]
    fn nats_conf_rejects_each_c0_del_and_c1_character_wherever_it_stands() {
        let controls = (0u32..=0x1f).chain(0x7f..=0x9f);
        for code in controls {
            let c = char::from_u32(code).expect("a control code point is a char");
            for text in [
                format!("{c}"),
                format!("a{c}"),
                format!("{c}a"),
                format!("\"{c}\\"),
            ] {
                assert_eq!(
                    TemplateFormat::NatsConf.escape_text(&text),
                    None,
                    "U+{code:04X} in {text:?}"
                );
            }
        }
    }

    // covers: AC-2 (spec 0039)
    #[test]
    fn nats_conf_keeps_characters_next_to_the_control_ranges_and_outside_the_bmp() {
        for text in [
            "\u{20}", "\u{7e}", "\u{a0}", "\u{2028}", "\u{feff}", "🔑", "a\u{301}",
        ] {
            assert_eq!(
                TemplateFormat::NatsConf.escape_text(text).unwrap(),
                format!("\"{text}\""),
                "{text:?}"
            );
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

    // covers: AC-2 (spec 0040)
    #[test]
    fn valkey_conf_double_quotes_text_and_escapes_quotes_and_backslashes() {
        let format = TemplateFormat::ValkeyConf;

        assert_eq!(format.escape_text("none").unwrap(), "\"none\"");
        assert_eq!(
            format.escape_text("a\"b\\c d ü #x").unwrap(),
            "\"a\\\"b\\\\c d ü #x\""
        );
        assert_eq!(format.escape_text("").unwrap(), "\"\"");
        assert_eq!(
            format.escape_text("$HOME 'q' `id` {x}").unwrap(),
            "\"$HOME 'q' `id` {x}\""
        );
    }

    // covers: AC-2 (spec 0040)
    #[test]
    fn valkey_conf_rejects_every_control_character() {
        for bad in [
            "a\nb", "a\tb", "a\rb", "a\u{0}", "a\u{1f}", "a\u{7f}", "a\u{85}", "a\u{9f}",
        ] {
            assert_eq!(TemplateFormat::ValkeyConf.escape_text(bad), None, "{bad:?}");
        }
        assert!(TemplateFormat::ValkeyConf.escape_text("a\u{a0}b").is_some());
    }

    // covers: AC-2 (spec 0040)
    #[test]
    fn valkey_conf_writes_booleans_as_yes_and_no_and_takes_tags_without_a_parse_back() {
        let format = TemplateFormat::ValkeyConf;

        assert_eq!(format.boolean(true), "yes");
        assert_eq!(format.boolean(false), "no");
        assert!(format.allows_tags());
        assert!(format.output_parses("port 6379\nunbalanced \"\n"));
        assert_eq!(
            TemplateFormat::from_code("valkeyConf"),
            Some(TemplateFormat::ValkeyConf)
        );
        assert_eq!(TemplateFormat::from_code("valkey"), None);
        assert_eq!(TemplateFormat::from_code("ValkeyConf"), None);
    }

    /// Reads one double quoted argument the way Valkey's `sdssplitargs`
    /// does for a config line: `\xHH` is a byte, `\n`, `\r`, `\t`, `\b`, and
    /// `\a` are control characters, any other `\c` is `c`, and the closing
    /// quote must end the line or come before a space. `None` for a line
    /// Valkey would refuse or split into more than one argument.
    fn valkey_unquote(line: &str) -> Option<String> {
        let mut bytes = line.as_bytes().iter().copied().peekable();
        if bytes.next() != Some(b'"') {
            return None;
        }
        let mut out = Vec::new();
        loop {
            match bytes.next()? {
                b'\\' => match bytes.next()? {
                    b'x' => {
                        let high = bytes.next()?;
                        let low = bytes.next()?;
                        let text = [high, low];
                        let hex = std::str::from_utf8(&text).ok()?;
                        out.push(u8::from_str_radix(hex, 16).ok()?);
                    }
                    b'n' => out.push(b'\n'),
                    b'r' => out.push(b'\r'),
                    b't' => out.push(b'\t'),
                    b'b' => out.push(0x08),
                    b'a' => out.push(0x07),
                    other => out.push(other),
                },
                b'"' => {
                    return match bytes.next() {
                        None => String::from_utf8(out).ok(),
                        Some(_) => None,
                    };
                }
                other => out.push(other),
            }
        }
    }

    // covers: AC-2 (spec 0040)
    #[test]
    fn valkey_conf_text_reads_back_as_one_unchanged_argument_for_every_edge_mix() {
        for text in [
            "",
            "\\",
            "\"",
            "\\\"",
            "\"\\",
            "a\\",
            "\\\\",
            "\"\"",
            "a\"\"b",
            "\\x41",
            "\\x4",
            "\\n\\r\\t",
            "$HOME",
            "${x}",
            "$",
            "#x",
            "# x",
            " # ",
            "  ",
            " a ",
            "'",
            "''",
            "{{ x }}",
            "a\\\\\"b",
            "\\\" #$",
            "ü 空 \u{1f600}",
            "\u{a0}\u{2028}\u{2029}\u{feff}",
            "a\"b\\c d ü #x",
        ] {
            let escaped = TemplateFormat::ValkeyConf
                .escape_text(text)
                .unwrap_or_else(|| panic!("{text:?} is representable"));
            assert_eq!(
                valkey_unquote(&escaped).as_deref(),
                Some(text),
                "{text:?} as {escaped}"
            );
        }
    }

    // covers: AC-2 (spec 0040)
    #[test]
    fn valkey_conf_rejects_each_c0_del_and_c1_character_wherever_it_stands() {
        let controls = (0x00..=0x1f_u32).chain(0x7f..=0x9f);
        for code in controls {
            let c = char::from_u32(code).expect("a control code point is a char");
            for text in [
                format!("{c}"),
                format!("{c}a"),
                format!("a{c}"),
                format!("a{c}b"),
                format!("\"{c}\\"),
            ] {
                assert_eq!(
                    TemplateFormat::ValkeyConf.escape_text(&text),
                    None,
                    "U+{code:04X} in {text:?}"
                );
            }
        }
        for code in [0x20_u32, 0x7e, 0xa0, 0xff] {
            let c = char::from_u32(code).expect("a printable code point is a char");
            assert!(
                TemplateFormat::ValkeyConf
                    .escape_text(&format!("a{c}b"))
                    .is_some(),
                "U+{code:04X} is kept"
            );
        }
    }

    #[test]
    fn mysql_cnf_double_quotes_text_and_escapes_quotes_and_backslashes() {
        let format = TemplateFormat::MysqlCnf;

        assert_eq!(
            format.escape_text("a\"b\\c d # e ü 空 'q' ; z").unwrap(),
            "\"a\\\"b\\\\c d # e ü 空 'q' ; z\""
        );
        assert_eq!(format.escape_text("").unwrap(), "\"\"");
        assert_eq!(format.escape_text("$HOME `id`").unwrap(), "\"$HOME `id`\"");
    }

    #[test]
    fn mysql_cnf_rejects_every_control_character() {
        for bad in [
            "a\nb", "a\tb", "a\rb", "a\u{0}", "a\u{1f}", "a\u{7f}", "a\u{85}", "a\u{9f}",
        ] {
            assert_eq!(TemplateFormat::MysqlCnf.escape_text(bad), None, "{bad:?}");
        }
        assert!(TemplateFormat::MysqlCnf.escape_text("a\u{a0}b").is_some());
    }

    #[test]
    fn mysql_cnf_writes_booleans_as_on_and_off_and_takes_tags_without_a_parse_back() {
        let format = TemplateFormat::MysqlCnf;

        assert_eq!(format.boolean(true), "ON");
        assert_eq!(format.boolean(false), "OFF");
        assert!(format.allows_tags());
        assert!(format.output_parses("[mysqld]\nunbalanced = \"\n"));
        assert_eq!(
            TemplateFormat::from_code("mysqlCnf"),
            Some(TemplateFormat::MysqlCnf)
        );
        assert_eq!(TemplateFormat::from_code("mysql"), None);
        assert_eq!(TemplateFormat::from_code("MysqlCnf"), None);
    }

    #[test]
    fn xml_writes_entities_for_markup_characters_and_no_quotes() {
        let format = TemplateFormat::Xml;

        assert_eq!(
            format.escape_text("/Volumes/T/data").unwrap(),
            "/Volumes/T/data"
        );
        assert_eq!(
            format.escape_text("a&b<c>d\"e'f ü 空 $x {y}").unwrap(),
            "a&amp;b&lt;c&gt;d&quot;e&apos;f ü 空 $x {y}"
        );
        assert_eq!(format.escape_text("&amp;").unwrap(), "&amp;amp;");
        assert_eq!(format.escape_text("").unwrap(), "");
    }

    #[test]
    fn xml_rejects_control_characters_and_the_two_noncharacters() {
        for bad in [
            "a\nb",
            "a\tb",
            "a\rb",
            "a\u{0}",
            "a\u{1f}",
            "a\u{7f}",
            "a\u{85}",
            "a\u{9f}",
            "a\u{fffe}",
            "a\u{ffff}",
        ] {
            assert_eq!(TemplateFormat::Xml.escape_text(bad), None, "{bad:?}");
        }
        for good in ["a\u{a0}b", "a\u{fffd}b", "a\u{1f600}b"] {
            assert!(TemplateFormat::Xml.escape_text(good).is_some(), "{good:?}");
        }
    }

    // covers: AC-1 (spec 0043)
    #[test]
    fn xml_rejects_every_c0_and_c1_control_and_takes_the_characters_around_them() {
        let refused: Vec<char> = ('\u{0}'..='\u{1f}')
            .chain('\u{7f}'..='\u{9f}')
            .chain(['\u{fffe}', '\u{ffff}'])
            .collect();
        assert_eq!(refused.len(), 32 + 33 + 2);
        for c in refused {
            let text = format!("a{c}b");
            assert_eq!(TemplateFormat::Xml.escape_text(&text), None, "{c:?}");
            assert_eq!(
                TemplateFormat::Xml.escape_text(&c.to_string()),
                None,
                "{c:?}"
            );
        }
        for c in [
            '\u{20}',
            '\u{7e}',
            '\u{a0}',
            '\u{fffd}',
            '\u{10000}',
            '\u{10ffff}',
        ] {
            let text = format!("a{c}b");
            assert_eq!(
                TemplateFormat::Xml.escape_text(&text).as_deref(),
                Some(text.as_str()),
                "{c:?}"
            );
        }
    }

    // covers: AC-1 (spec 0043)
    #[test]
    fn xml_writes_each_of_the_five_markup_characters_as_its_own_entity() {
        for (raw, entity) in [
            ("&", "&amp;"),
            ("<", "&lt;"),
            (">", "&gt;"),
            ("\"", "&quot;"),
            ("'", "&apos;"),
        ] {
            assert_eq!(
                TemplateFormat::Xml.escape_text(raw).as_deref(),
                Some(entity),
                "{raw}"
            );
            assert_eq!(
                TemplateFormat::Xml.escape_text(&format!("{raw}{raw}")),
                Some(format!("{entity}{entity}")),
                "{raw} twice"
            );
        }
        assert_eq!(
            TemplateFormat::Xml.escape_text("<![CDATA[x]]>").as_deref(),
            Some("&lt;![CDATA[x]]&gt;"),
            "a CDATA section in a value stays text"
        );
        assert_eq!(
            TemplateFormat::Xml.escape_text("&#65;").as_deref(),
            Some("&amp;#65;"),
            "a character reference in a value stays text"
        );
    }

    #[test]
    fn xml_writes_booleans_as_true_and_false_and_takes_tags_without_a_parse_back() {
        let format = TemplateFormat::Xml;

        assert_eq!(format.boolean(true), "true");
        assert_eq!(format.boolean(false), "false");
        assert!(format.allows_tags());
        assert!(format.output_parses("<clickhouse><unclosed></clickhouse>"));
        assert_eq!(TemplateFormat::from_code("xml"), Some(TemplateFormat::Xml));
        assert_eq!(TemplateFormat::from_code("XML"), None);
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
