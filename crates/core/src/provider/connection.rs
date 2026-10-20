//! The manifest `connection` block (spec 0021): named URI templates a
//! provider declares so clients can show how to connect to an instance.
//!
//! The block is checked when the package loads: names, titles, lengths,
//! the tokens of the `connection.template` position (spec 0009), and a
//! render with sentinel values that must give an absolute RFC 3986 URI.
//! Every token value is percent encoded, except endpoint ports and
//! addresses, so a value can never change the structure of the URI.

use std::collections::BTreeSet;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::path::PathBuf;

use stackly_domain::Port;

use super::interpolate::{Interp, Piece, Token, TypeEnv, check_connection_template};
use super::issue::{FileIssues, Issue, IssueCode, Issues, Location};
use super::manifest::Manifest;
use super::package::MANIFEST_FILE;
use super::plan::{PlanError, ResolvedEndpoint, Resolver, Value};
use super::raw::RawConnection;
use super::settings::SettingsSchema;
use super::validate::sample_context;

/// Most entries one `connection` block holds.
pub const CONNECTION_ENTRIES_MAX: usize = 8;
/// Longest `template`, in bytes.
pub const CONNECTION_TEMPLATE_MAX: usize = 512;
/// Longest `title`, in characters.
pub const CONNECTION_TITLE_MAX: usize = 64;
/// Longest `name`, in bytes.
const CONNECTION_NAME_MAX: usize = 32;

/// The reasons of a `connection_invalid` issue, carried in `Issue::reason`.
pub(crate) mod reason {
    /// The name breaks `^[a-z][a-z0-9_]{0,31}$`.
    pub(crate) const NAME: &str = "name";
    /// The title is empty, too long, or holds a control character.
    pub(crate) const TITLE: &str = "title";
    /// The template is empty or longer than 512 bytes.
    pub(crate) const TEMPLATE_LENGTH: &str = "template_length";
    /// With sentinel values the template does not render to an absolute
    /// URI.
    pub(crate) const NOT_A_URI: &str = "not_a_uri";
}

/// The port every IP endpoint gets in the sentinel render.
const SENTINEL_PORT: u16 = 15432;
/// The hosts the sentinel render tries, IPv4 and IPv6 loopback.
const SENTINEL_HOSTS: [IpAddr; 2] = [
    IpAddr::V4(Ipv4Addr::LOCALHOST),
    IpAddr::V6(Ipv6Addr::LOCALHOST),
];
/// The run folder of the sentinel render, with a space so that a token
/// value which is not encoded breaks the URI. It sits under the same
/// `/stackly` base as the sample render, because library code never names
/// a machine folder such as the system temp folder (`cargo xtask check-deps`).
const SENTINEL_RUN_DIR: &str = if cfg!(windows) {
    r"C:\stackly sentinel\run"
} else {
    "/stackly sentinel/run"
};
/// The value every secret gets in the sentinel render.
const SENTINEL_SECRET: &str = "sentinel value";

/// One checked `connection` entry: a name, a title for the GUI, and the
/// URI template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionSpec {
    pub(crate) name: String,
    pub(crate) title: String,
    pub(crate) template: Interp,
}

impl ConnectionSpec {
    /// Returns the entry name, unique in the manifest.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the label the GUI shows.
    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }
}

/// Checks the `connection` block and returns its entries in manifest
/// order. A package without the block has no entries.
pub(crate) fn check(
    raw: Option<&[RawConnection]>,
    env: &TypeEnv<'_>,
    issues: &mut FileIssues<'_>,
) -> Vec<ConnectionSpec> {
    let Some(raw) = raw else {
        return Vec::new();
    };
    if raw.is_empty() || raw.len() > CONNECTION_ENTRIES_MAX {
        issues.at(
            "connection",
            IssueCode::InvalidValue,
            format!("connection holds 1 to {CONNECTION_ENTRIES_MAX} entries"),
        );
    }
    let mut seen = BTreeSet::new();
    let mut entries = Vec::new();
    for (index, entry) in raw.iter().enumerate() {
        let start = issues.count();
        let path = format!("connection[{index}]");
        if !is_connection_name(&entry.name) {
            issues.at_with_reason(
                format!("{path}.name"),
                IssueCode::ConnectionInvalid,
                reason::NAME,
                "name must be a lowercase letter, then at most 31 lowercase letters, digits, or _",
            );
        } else if !seen.insert(entry.name.as_str()) {
            issues.at(
                format!("{path}.name"),
                IssueCode::ConnectionNameDuplicate,
                format!("the connection name {} is used twice", entry.name),
            );
        }
        let title_len = entry.title.chars().count();
        if !(1..=CONNECTION_TITLE_MAX).contains(&title_len)
            || entry.title.chars().any(char::is_control)
        {
            issues.at_with_reason(
                format!("{path}.title"),
                IssueCode::ConnectionInvalid,
                reason::TITLE,
                format!(
                    "title must be 1 to {CONNECTION_TITLE_MAX} characters without control characters"
                ),
            );
        }
        let template_path = format!("{path}.template");
        let template = if (1..=CONNECTION_TEMPLATE_MAX).contains(&entry.template.len()) {
            check_connection_template(&entry.template, env, issues, &template_path)
        } else {
            issues.at_with_reason(
                template_path,
                IssueCode::ConnectionInvalid,
                reason::TEMPLATE_LENGTH,
                format!("template must be 1 to {CONNECTION_TEMPLATE_MAX} bytes"),
            );
            None
        };
        if let Some(template) = template
            && issues.count() == start
        {
            entries.push(ConnectionSpec {
                name: entry.name.clone(),
                title: entry.title.clone(),
                template,
            });
        }
    }
    entries
}

/// Whether `name` matches `^[a-z][a-z0-9_]{0,31}$`.
fn is_connection_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    matches!(bytes.first(), Some(b'a'..=b'z'))
        && bytes.len() <= CONNECTION_NAME_MAX
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'_')
}

/// Renders every connection of a valid manifest with sentinel values, once
/// with each loopback host, and reports a template whose result is not an
/// absolute RFC 3986 URI as `connection_invalid` with reason `not_a_uri`.
pub(crate) fn check_sentinels(manifest: &Manifest, settings: &SettingsSchema, issues: &mut Issues) {
    if manifest.connections.is_empty() {
        return;
    }
    let mut reported = BTreeSet::new();
    for host in SENTINEL_HOSTS {
        let Some(mut context) = sample_context(manifest, settings, false) else {
            return;
        };
        let port = Port::new(SENTINEL_PORT).ok();
        for endpoint in context.endpoints.values_mut() {
            if let ResolvedEndpoint::Ip { .. } = endpoint {
                *endpoint = ResolvedEndpoint::Ip { host, port };
            }
        }
        context.paths.run = PathBuf::from(SENTINEL_RUN_DIR);
        let resolver = match Resolver::new(manifest, &context) {
            Ok(resolver) => resolver,
            Err(err) => {
                issues.push(Issue::new(
                    MANIFEST_FILE,
                    Location::Field("connection".to_owned()),
                    IssueCode::InvalidValue,
                    format!("sentinel values do not resolve: {err}"),
                ));
                return;
            }
        };
        let mut yaml = FileIssues::new(MANIFEST_FILE, issues);
        for (index, connection) in manifest.connections.iter().enumerate() {
            if reported.contains(&index) {
                continue;
            }
            let path = format!("connection[{index}].template");
            match render_sentinel(&connection.template, &resolver) {
                Ok(text) if is_absolute_uri(&text) => {}
                Ok(_) => {
                    reported.insert(index);
                    yaml.at_with_reason(
                        path,
                        IssueCode::ConnectionInvalid,
                        reason::NOT_A_URI,
                        format!(
                            "with sentinel values and host {host} the template does not render to an absolute uri (RFC 3986)"
                        ),
                    );
                }
                Err(err) => {
                    reported.insert(index);
                    yaml.at(
                        path,
                        IssueCode::InvalidValue,
                        format!("the template does not render with sentinel values: {err}"),
                    );
                }
            }
        }
    }
}

/// Renders a template with the resolver's values, every secret as the
/// sentinel secret.
fn render_sentinel(template: &Interp, resolver: &Resolver<'_>) -> Result<String, PlanError> {
    let mut out = String::new();
    for piece in &template.pieces {
        match piece {
            Piece::Literal(text) => out.push_str(text),
            Piece::Token(Token::Secret(_)) => out.push_str(&encode_value(SENTINEL_SECRET)),
            Piece::Token(token) => match resolver.value(token)? {
                Value::Text(text) if is_inserted_raw(token) => out.push_str(&text),
                Value::Text(text) => out.push_str(&encode_value(&text)),
                Value::Integer(number) => out.push_str(&number.to_string()),
                Value::Boolean(flag) => out.push_str(if flag { "true" } else { "false" }),
                Value::Secret(_) => out.push_str(&encode_value(SENTINEL_SECRET)),
            },
        }
    }
    Ok(out)
}

/// Whether a connection token's value goes into the URI as is: endpoint
/// ports and addresses (an IPv6 address in square brackets).
pub(crate) const fn is_inserted_raw(token: &Token) -> bool {
    matches!(token, Token::EndpointPort(_) | Token::EndpointAddress(_))
}

/// Percent encodes a token value as UTF-8 bytes with uppercase hex, except
/// the RFC 3986 unreserved characters `A-Z a-z 0-9 - . _ ~` (spec 0021).
pub(crate) fn encode_value(text: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if is_unreserved(byte) {
            out.push(char::from(byte));
        } else {
            out.push('%');
            out.push(char::from(HEX[usize::from(byte >> 4)]));
            out.push(char::from(HEX[usize::from(byte & 0x0F)]));
        }
    }
    out
}

/// Whether `text` is an RFC 3986 `absolute-URI`: a scheme, `:`, a
/// hierarchical part, and an optional query, with no fragment.
pub(crate) fn is_absolute_uri(text: &str) -> bool {
    let Some((scheme, rest)) = text.split_once(':') else {
        return false;
    };
    if !is_scheme(scheme) {
        return false;
    }
    let (hier, query) = match rest.split_once('?') {
        Some((hier, query)) => (hier, Some(query)),
        None => (rest, None),
    };
    if query.is_some_and(|query| !uri_chars(query, b":@/?")) {
        return false;
    }
    match hier.strip_prefix("//") {
        Some(after) => {
            let (authority, path) = after.find('/').map_or((after, ""), |at| after.split_at(at));
            is_authority(authority) && is_path(path)
        }
        None => is_path(hier),
    }
}

/// `scheme = ALPHA *( ALPHA / DIGIT / "+" / "-" / "." )`.
fn is_scheme(scheme: &str) -> bool {
    let bytes = scheme.as_bytes();
    matches!(bytes.first(), Some(b) if b.is_ascii_alphabetic())
        && bytes
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'-' | b'.'))
}

/// `authority = [ userinfo "@" ] host [ ":" port ]`.
fn is_authority(authority: &str) -> bool {
    let (userinfo, host_port) = match authority.split_once('@') {
        Some((userinfo, host_port)) => (Some(userinfo), host_port),
        None => (None, authority),
    };
    if userinfo.is_some_and(|userinfo| !uri_chars(userinfo, b":")) {
        return false;
    }
    let (host_ok, port) = if let Some(literal) = host_port.strip_prefix('[') {
        let Some((inside, after)) = literal.split_once(']') else {
            return false;
        };
        let port = match after.strip_prefix(':') {
            Some(port) => port,
            None if after.is_empty() => "",
            None => return false,
        };
        (is_ip_literal(inside), port)
    } else {
        let (host, port) = host_port.split_once(':').unwrap_or((host_port, ""));
        (uri_chars(host, b""), port)
    };
    host_ok && port.bytes().all(|b| b.is_ascii_digit())
}

/// The inside of `IP-literal`: an IPv6 address or `IPvFuture`.
fn is_ip_literal(inside: &str) -> bool {
    if inside.parse::<Ipv6Addr>().is_ok() {
        return true;
    }
    let Some(future) = inside.strip_prefix(['v', 'V']) else {
        return false;
    };
    let Some((version, rest)) = future.split_once('.') else {
        return false;
    };
    !version.is_empty()
        && version.bytes().all(|b| b.is_ascii_hexdigit())
        && !rest.is_empty()
        && rest
            .bytes()
            .all(|b| is_unreserved(b) || is_sub_delim(b) || b == b':')
}

/// A path of `pchar` segments separated by `/`, which covers
/// `path-abempty`, `path-absolute`, `path-rootless`, and `path-empty` once
/// the authority case is split off.
fn is_path(path: &str) -> bool {
    path.split('/').all(|segment| uri_chars(segment, b":@"))
}

/// Whether every byte is unreserved, a sub-delim, one of `extra`, or part
/// of a `%XX` escape.
fn uri_chars(text: &str, extra: &[u8]) -> bool {
    let mut bytes = text.bytes();
    while let Some(byte) = bytes.next() {
        let ok = if byte == b'%' {
            matches!((bytes.next(), bytes.next()), (Some(a), Some(b)) if a.is_ascii_hexdigit() && b.is_ascii_hexdigit())
        } else {
            is_unreserved(byte) || is_sub_delim(byte) || extra.contains(&byte)
        };
        if !ok {
            return false;
        }
    }
    true
}

/// `unreserved = ALPHA / DIGIT / "-" / "." / "_" / "~"`.
const fn is_unreserved(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~')
}

/// `sub-delims = "!" / "$" / "&" / "'" / "(" / ")" / "*" / "+" / "," / ";" / "="`.
const fn is_sub_delim(byte: u8) -> bool {
    matches!(
        byte,
        b'!' | b'$' | b'&' | b'\'' | b'(' | b')' | b'*' | b'+' | b',' | b';' | b'='
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_are_percent_encoded_as_utf8_with_uppercase_hex() {
        assert_eq!(encode_value("sentinel value"), "sentinel%20value");
        assert_eq!(encode_value("a-b.c_d~e"), "a-b.c_d~e");
        assert_eq!(encode_value("/data/x"), "%2Fdata%2Fx");
        assert_eq!(encode_value("p@ss:w/rd?#%"), "p%40ss%3Aw%2Frd%3F%23%25");
        assert_eq!(encode_value("é"), "%C3%A9");
        assert_eq!(encode_value("😀"), "%F0%9F%98%80");
    }

    #[test]
    fn connection_names_follow_the_pattern() {
        assert!(is_connection_name("uri"));
        assert!(is_connection_name("socket_2"));
        assert!(is_connection_name(&"a".repeat(32)));
        assert!(!is_connection_name(""));
        assert!(!is_connection_name("2uri"));
        assert!(!is_connection_name("Uri"));
        assert!(!is_connection_name("my-uri"));
        assert!(!is_connection_name(&"a".repeat(33)));
    }

    #[test]
    fn provider_connection_strings_are_absolute_uris() {
        for uri in [
            "postgresql://postgres:sentinel%20value@127.0.0.1:15432/postgres",
            "postgresql://postgres:sentinel%20value@[::1]:15432/postgres",
            "postgresql://postgres:sentinel%20value@/postgres?host=%2Fstackly%20sentinel%2Frun&port=15432",
            "nats://127.0.0.1:15432",
            "http://127.0.0.1:15432",
            "http://stackly:sentinel%20value@[::1]:15432/",
            "redis://:sentinel%20value@127.0.0.1:15432/0",
            "mailto:someone@example.com",
            "urn:example:a",
            "x://[v1.fe80::a]:1",
            "file:///var/a",
        ] {
            assert!(is_absolute_uri(uri), "{uri}");
        }
    }

    #[test]
    fn relative_references_spaces_fragments_and_bad_parts_are_not_absolute_uris() {
        for text in [
            "",
            "127.0.0.1:15432",
            "//127.0.0.1:15432",
            "/postgres",
            "postgres",
            "1http://h",
            "postgresql://h/a b",
            "postgresql://h/a#frag",
            "postgresql://u:p@ss@h",
            "postgresql://[::1/x",
            "postgresql://[::1]x",
            "postgresql://[nothost]",
            "postgresql://h:12a",
            "postgresql://h/%zz",
            "postgresql://h/%2",
            "postgresql://h/é",
            "postgresql://h?host=/stackly sentinel/run",
            "post gres://h",
        ] {
            assert!(!is_absolute_uri(text), "{text}");
        }
    }
}
