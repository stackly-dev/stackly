//! The runtime inventory `runtimes/inventory.toml` (spec 0003) and rules 1
//! to 6 of `cargo xtask runtime check`.
//!
//! Every field is read as written, with enum values kept as strings, so a
//! wrong value becomes one `<id>: <rule>` line instead of a TOML error.
//! Unknown keys are refused, so a typo never hides a field.
// provisional: F9 (features 9 and 18 decide how the catalog reads this data)

use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;
use stackly_evidence::{is_exact_version, is_sha256_hex};
use toml::value::Datetime;

/// The inventory, relative to the repository root.
pub const INVENTORY_FILE: &str = "runtimes/inventory.toml";
/// The folder for upstream GPG keys, relative to the repository root.
pub const KEYS_DIR: &str = "runtimes/keys";

/// The providers Stackly ships a runtime for.
const PROVIDERS: [&str; 6] = [
    "postgresql",
    "mysql",
    "nats",
    "clickhouse",
    "valkey",
    "traefik",
];
/// The prefixes a relocatable install name may use.
const RELATIVE_INSTALL_PREFIXES: [&str; 3] = ["@rpath/", "@loader_path/", "@executable_path/"];
/// The folders a system library may live in.
const SYSTEM_INSTALL_PREFIXES: [&str; 2] = ["/usr/lib/", "/System/Library/"];

/// The whole `runtimes/inventory.toml`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Inventory {
    /// Always `1`.
    pub schema_version: Option<i64>,
    /// Every `[[artifact]]`, in file order.
    #[serde(default, rename = "artifact")]
    pub artifacts: Vec<Artifact>,
}

/// One `[[artifact]]`: one runtime line of one provider.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Artifact {
    /// `<provider>-<version>[-r<build>]-<os>-<arch>`.
    pub id: Option<String>,
    /// One of [`PROVIDERS`].
    pub provider: Option<String>,
    /// The release line, for example `17` or `2.15`.
    pub line: Option<String>,
    /// The exact version, starting with `<line>.`.
    pub version: Option<String>,
    /// The maintainer build number, from 1.
    pub build: Option<i64>,
    /// Always `macos`.
    pub os: Option<String>,
    /// Always `aarch64`.
    pub arch: Option<String>,
    /// The minimum macOS version of the main executable.
    pub min_os: Option<String>,
    /// `available` or `blocked`.
    pub status: Option<String>,
    /// Why the line has no managed artifact.
    pub blocked_reason: Option<String>,
    /// Who can unblock it, as `@handle`.
    pub blocked_owner: Option<String>,
    /// Since when, as a TOML date.
    pub blocked_since: Option<Datetime>,
    /// `upstream` or `maintainer`.
    pub origin: Option<String>,
    /// `upstream_download` or `stackly_hosted`.
    pub distribution: Option<String>,
    /// `tar_gz` or `binary`.
    pub format: Option<String>,
    /// Candidate download URLs, in order; the digest decides identity.
    pub urls: Option<Vec<String>>,
    /// The SHA-256 of the download, 64 lowercase hex characters.
    pub sha256: Option<String>,
    /// The size of the download in bytes.
    pub size: Option<i64>,
    /// The single top folder of the archive, when it has one.
    #[expect(
        dead_code,
        reason = "the spike extracts with it (spec 0003 build step 6)"
    )]
    pub archive_root: Option<String>,
    /// The checksum file upstream publishes.
    pub upstream_checksum: Option<UpstreamChecksum>,
    /// The signature upstream publishes.
    pub upstream_signature: Option<UpstreamSignature>,
    /// The upstream source tarball a maintainer build starts from.
    pub source_url: Option<String>,
    /// The SHA-256 of that source tarball.
    pub source_sha256: Option<String>,
    /// `ojiepermana/stackly-runtimes@<commit>:recipes/<provider>.sh`.
    pub recipe: Option<String>,
    /// The GitHub attestation URL of a maintainer build.
    pub attestation: Option<String>,
    /// `keep_or_adhoc` or `adhoc_pipeline`.
    pub signing_policy: Option<String>,
    /// The SPDX license expression.
    pub license: Option<String>,
    /// Where the license text lives.
    pub license_url: Option<String>,
    /// Paths inside the artifact root; the main executable comes first.
    pub executables: Option<Vec<String>>,
    /// Every non system and system library the artifact links.
    #[serde(default, rename = "dependency")]
    pub dependencies: Vec<Dependency>,
}

/// `upstream_checksum = { kind, url }`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpstreamChecksum {
    /// `sha256sums`, `checksums_txt`, `sha512`, or `none`.
    pub kind: Option<String>,
    /// The checksum file; required unless `kind` is `none`.
    pub url: Option<String>,
}

/// `upstream_signature = { kind, url, key, fingerprint }`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpstreamSignature {
    /// `gpg` or `none`.
    pub kind: Option<String>,
    /// The detached signature (`.asc`).
    pub url: Option<String>,
    /// The public key file, `runtimes/keys/<file>`.
    pub key: Option<String>,
    /// The fingerprint of that key, 40 or 64 hex characters.
    pub fingerprint: Option<String>,
}

/// One `[[artifact.dependency]]`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Dependency {
    /// Unique within the artifact, for example `libssl.3.dylib`.
    pub name: Option<String>,
    /// `bundled`, `own`, or `system`.
    pub kind: Option<String>,
    /// Exactly as `otool -L` prints it.
    pub install_name: Option<String>,
    /// Bundled only: the exact version.
    pub version: Option<String>,
    /// Bundled only: the SPDX license.
    pub license: Option<String>,
    /// Bundled only: where the source came from.
    pub source_url: Option<String>,
}

/// One broken rule, printed as `<subject>: <message> (rule <n>)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    /// The artifact ID, `artifact #<n>` when it has none, `inventory`, or
    /// an evidence folder.
    pub subject: String,
    /// The rule number in spec 0003, *Key invariants*.
    pub rule: u8,
    /// What is wrong, as a lowercase sentence without a trailing period.
    pub message: String,
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {} (rule {})", self.subject, self.message, self.rule)
    }
}

impl Inventory {
    /// Reads and decodes `runtimes/inventory.toml` under `root`.
    pub fn load(root: &Path) -> Result<Self> {
        let path = root.join(INVENTORY_FILE);
        let text =
            fs::read_to_string(&path).with_context(|| format!("cannot read {}", path.display()))?;
        Self::parse(&text).with_context(|| format!("{INVENTORY_FILE} is not a valid inventory"))
    }

    /// Decodes inventory TOML.
    pub fn parse(text: &str) -> Result<Self> {
        Ok(toml::from_str(text)?)
    }

    /// Checks rules 1 to 6. `root` is the repository root, where GPG key
    /// files are looked up.
    pub fn violations(&self, root: &Path) -> Vec<Violation> {
        let mut out = Vec::new();
        if self.schema_version != Some(1) {
            out.push(Violation {
                subject: "inventory".to_owned(),
                rule: 1,
                message: "schema_version must be 1".to_owned(),
            });
        }
        let mut ids: HashSet<&str> = HashSet::new();
        let mut lines: HashMap<(&str, &str, &str, &str), String> = HashMap::new();
        for (index, artifact) in self.artifacts.iter().enumerate() {
            let subject = artifact.label(index);
            let mut v = Rules {
                subject: &subject,
                out: &mut out,
            };
            if let Some(id) = filled(&artifact.id)
                && !ids.insert(id)
            {
                v.push(1, "id is used by more than one artifact".to_owned());
            }
            if let (Some(provider), Some(line), Some(os), Some(arch)) = (
                filled(&artifact.provider),
                filled(&artifact.line),
                filled(&artifact.os),
                filled(&artifact.arch),
            ) {
                match lines.entry((provider, line, os, arch)) {
                    Entry::Occupied(first) => v.push(
                        1,
                        format!(
                            "provider, line, os, and arch repeat those of {}",
                            first.get()
                        ),
                    ),
                    Entry::Vacant(slot) => {
                        slot.insert(subject.clone());
                    }
                }
            }
            artifact.check(root, &mut v);
        }
        out
    }

    /// The artifacts whose status is `available`.
    pub fn available(&self) -> impl Iterator<Item = &Artifact> {
        self.artifacts
            .iter()
            .filter(|artifact| artifact.status.as_deref() == Some("available"))
    }
}

/// Collects the violations of one artifact.
struct Rules<'a> {
    subject: &'a str,
    out: &'a mut Vec<Violation>,
}

impl Rules<'_> {
    fn push(&mut self, rule: u8, message: String) {
        self.out.push(Violation {
            subject: self.subject.to_owned(),
            rule,
            message,
        });
    }

    fn check(&mut self, rule: u8, ok: bool, message: impl FnOnce() -> String) {
        if !ok {
            self.push(rule, message());
        }
    }

    /// `field` must be present and not blank.
    fn require(&mut self, rule: u8, field: &str, value: &Option<String>, when: &str) {
        self.check(rule, filled(value).is_some(), || {
            format!("{when} needs {field}")
        });
    }

    /// `field`, when present, must be one of `allowed`.
    fn one_of(&mut self, rule: u8, field: &str, value: &Option<String>, allowed: &[&str]) {
        if let Some(value) = value.as_deref() {
            self.check(rule, allowed.contains(&value), || {
                format!("{field} must be {}, found `{value}`", or_list(allowed))
            });
        }
    }

    /// `field` must be present and an `https://` URL.
    fn require_https(&mut self, rule: u8, field: &str, value: &Option<String>, when: &str) {
        match filled(value) {
            None => self.push(rule, format!("{when} needs {field}")),
            Some(url) => self.check(rule, is_https(url), || {
                format!("{field} must be an https:// URL, found `{url}`")
            }),
        }
    }

    /// `field` must be present and equal to `expected`.
    fn require_eq(
        &mut self,
        rule: u8,
        field: &str,
        value: &Option<String>,
        expected: &str,
        when: &str,
    ) {
        self.check(rule, value.as_deref() == Some(expected), || {
            format!("{when} needs {field} = \"{expected}\"")
        });
    }
}

impl Artifact {
    /// The name violations use: the ID, or `artifact #<n>` (from 1).
    pub fn label(&self, index: usize) -> String {
        filled(&self.id).map_or_else(|| format!("artifact #{}", index + 1), str::to_owned)
    }

    fn check(&self, root: &Path, v: &mut Rules<'_>) {
        self.check_identity(v);
        self.check_version(v);
        match self.status.as_deref() {
            Some("available") => self.check_available(v),
            Some("blocked") => self.check_blocked(v),
            Some(other) => v.push(
                3,
                format!("status must be `available` or `blocked`, found `{other}`"),
            ),
            None => v.push(3, "status is required".to_owned()),
        }
        v.one_of(3, "format", &self.format, &["tar_gz", "binary"]);
        v.one_of(
            3,
            "signing_policy",
            &self.signing_policy,
            &["keep_or_adhoc", "adhoc_pipeline"],
        );
        self.check_origin(root, v);
        self.check_dependencies(v);
    }

    /// Rule 1: provider, and an ID derived from provider, version, build,
    /// os, and arch.
    fn check_identity(&self, v: &mut Rules<'_>) {
        v.require(1, "id", &self.id, "every artifact");
        match filled(&self.provider) {
            None => v.push(1, "every artifact needs provider".to_owned()),
            Some(provider) => v.check(1, PROVIDERS.contains(&provider), || {
                format!(
                    "provider must be {}, found `{provider}`",
                    or_list(&PROVIDERS)
                )
            }),
        }
        if let (Some(id), Some(expected)) = (filled(&self.id), self.expected_id()) {
            v.check(1, id == expected, || format!("id must be `{expected}`"));
        }
    }

    /// The ID rule 1 derives, when every part of it is present.
    fn expected_id(&self) -> Option<String> {
        let build = self
            .build
            .map_or_else(String::new, |build| format!("-r{build}"));
        Some(format!(
            "{}-{}{build}-{}-{}",
            filled(&self.provider)?,
            filled(&self.version)?,
            filled(&self.os)?,
            filled(&self.arch)?
        ))
    }

    /// Rule 2: an exact version inside its line, on macOS aarch64.
    fn check_version(&self, v: &mut Rules<'_>) {
        v.require(2, "line", &self.line, "every artifact");
        match filled(&self.version) {
            None => v.push(2, "every artifact needs version".to_owned()),
            Some(version) => {
                v.check(2, is_exact_version(version), || {
                    format!("version `{version}` is not exact")
                });
                if let Some(line) = filled(&self.line) {
                    v.check(2, version.starts_with(&format!("{line}.")), || {
                        format!("version `{version}` must start with `{line}.`")
                    });
                }
            }
        }
        v.require_eq(2, "os", &self.os, "macos", "every artifact");
        v.require_eq(2, "arch", &self.arch, "aarch64", "every artifact");
    }

    /// Rule 3: an `available` artifact can be downloaded, checked, and run.
    fn check_available(&self, v: &mut Rules<'_>) {
        const WHEN: &str = "an available artifact";
        match self.urls.as_deref() {
            None | Some([]) => v.push(3, format!("{WHEN} needs at least one URL in urls")),
            Some(urls) => {
                for url in urls {
                    v.check(3, is_https(url), || {
                        format!("urls must all be https:// URLs, found `{url}`")
                    });
                }
            }
        }
        v.check(3, self.sha256.as_deref().is_some_and(is_sha256_hex), || {
            format!("{WHEN} needs sha256 as 64 lowercase hex characters")
        });
        v.check(3, self.size.is_some_and(|size| size > 0), || {
            format!("{WHEN} needs size greater than 0")
        });
        v.require(3, "min_os", &self.min_os, WHEN);
        v.require(3, "format", &self.format, WHEN);
        v.require(3, "license", &self.license, WHEN);
        v.require_https(3, "license_url", &self.license_url, WHEN);
        v.require(3, "signing_policy", &self.signing_policy, WHEN);
        match self.executables.as_deref() {
            None | Some([]) => v.push(3, format!("{WHEN} needs at least one executable")),
            Some(executables) => {
                for path in executables {
                    v.check(3, is_inner_relative_path(path), || {
                        format!("executable `{path}` must be a plain relative path, without `..`, `.`, or empty parts")
                    });
                }
            }
        }
    }

    /// Rule 4: a `blocked` artifact says why, who, and since when.
    fn check_blocked(&self, v: &mut Rules<'_>) {
        const WHEN: &str = "a blocked artifact";
        v.require(4, "blocked_reason", &self.blocked_reason, WHEN);
        v.check(
            4,
            filled(&self.blocked_owner)
                .is_some_and(|owner| owner.len() > 1 && owner.starts_with('@')),
            || format!("{WHEN} needs blocked_owner as an @handle"),
        );
        v.check(
            4,
            self.blocked_since
                .as_ref()
                .is_some_and(|since| since.date.is_some() && since.time.is_none()),
            || format!("{WHEN} needs blocked_since as a date, for example 2026-10-17"),
        );
    }

    /// Rule 5: where the artifact comes from decides which fields it has.
    fn check_origin(&self, root: &Path, v: &mut Rules<'_>) {
        v.one_of(
            5,
            "distribution",
            &self.distribution,
            &["upstream_download", "stackly_hosted"],
        );
        match self.origin.as_deref() {
            Some("maintainer") => self.check_maintainer(v),
            Some("upstream") => self.check_upstream(root, v),
            Some(other) => v.push(
                5,
                format!("origin must be `upstream` or `maintainer`, found `{other}`"),
            ),
            None => v.push(5, "every artifact needs origin".to_owned()),
        }
    }

    fn check_maintainer(&self, v: &mut Rules<'_>) {
        const WHEN: &str = "a maintainer artifact";
        v.check(5, self.build.is_some_and(|build| build >= 1), || {
            format!("{WHEN} needs build of 1 or more")
        });
        v.require_https(5, "source_url", &self.source_url, WHEN);
        v.check(
            5,
            self.source_sha256.as_deref().is_some_and(is_sha256_hex),
            || format!("{WHEN} needs source_sha256 as 64 lowercase hex characters"),
        );
        v.require(5, "recipe", &self.recipe, WHEN);
        v.require_https(5, "attestation", &self.attestation, WHEN);
        v.require_eq(
            5,
            "distribution",
            &self.distribution,
            "stackly_hosted",
            WHEN,
        );
        v.require_eq(
            5,
            "signing_policy",
            &self.signing_policy,
            "adhoc_pipeline",
            WHEN,
        );
        v.require_eq(5, "format", &self.format, "tar_gz", WHEN);
    }

    fn check_upstream(&self, root: &Path, v: &mut Rules<'_>) {
        const WHEN: &str = "an upstream artifact";
        v.check(5, self.build.is_none(), || format!("{WHEN} has no build"));
        v.require_eq(
            5,
            "distribution",
            &self.distribution,
            "upstream_download",
            WHEN,
        );
        v.require_eq(
            5,
            "signing_policy",
            &self.signing_policy,
            "keep_or_adhoc",
            WHEN,
        );
        match &self.upstream_checksum {
            None => v.push(5, format!("{WHEN} needs upstream_checksum")),
            Some(checksum) => match checksum.kind.as_deref() {
                Some("none") => {}
                Some("sha256sums" | "checksums_txt" | "sha512") => v.require_https(
                    5,
                    "upstream_checksum.url",
                    &checksum.url,
                    "an upstream checksum",
                ),
                Some(other) => v.push(
                    5,
                    format!(
                        "upstream_checksum.kind must be `sha256sums`, `checksums_txt`, `sha512`, or `none`, found `{other}`"
                    ),
                ),
                None => v.push(5, "upstream_checksum needs kind".to_owned()),
            },
        }
        match &self.upstream_signature {
            None => v.push(5, format!("{WHEN} needs upstream_signature")),
            Some(signature) => match signature.kind.as_deref() {
                Some("none") => {}
                Some("gpg") => check_gpg(root, signature, v),
                Some(other) => v.push(
                    5,
                    format!("upstream_signature.kind must be `gpg` or `none`, found `{other}`"),
                ),
                None => v.push(5, "upstream_signature needs kind".to_owned()),
            },
        }
    }

    /// Rule 6: every dependency names its kind and a matching install name.
    fn check_dependencies(&self, v: &mut Rules<'_>) {
        let mut names: HashSet<&str> = HashSet::new();
        for (index, dependency) in self.dependencies.iter().enumerate() {
            let name = filled(&dependency.name);
            let label = name.map_or_else(
                || format!("dependency #{}", index + 1),
                |name| format!("dependency {name}"),
            );
            match name {
                None => v.push(6, format!("{label} needs name")),
                Some(name) => v.check(6, names.insert(name), || {
                    format!("{label} is listed more than once")
                }),
            }
            let install_name = filled(&dependency.install_name);
            v.check(6, install_name.is_some(), || {
                format!("{label} needs install_name")
            });
            let relative =
                install_name.is_some_and(|install| is_under(install, &RELATIVE_INSTALL_PREFIXES));
            match dependency.kind.as_deref() {
                Some("bundled") => {
                    v.require(6, "version", &dependency.version, &format!("bundled {label}"));
                    v.require(6, "license", &dependency.license, &format!("bundled {label}"));
                    v.require_https(
                        6,
                        "source_url",
                        &dependency.source_url,
                        &format!("bundled {label}"),
                    );
                    v.check(6, install_name.is_none() || relative, || {
                        format!(
                            "bundled {label} needs an install_name starting with @rpath/, @loader_path/, or @executable_path/"
                        )
                    });
                }
                Some("own") => v.check(6, install_name.is_none() || relative, || {
                    format!(
                        "own {label} needs an install_name starting with @rpath/, @loader_path/, or @executable_path/"
                    )
                }),
                Some("system") => v.check(
                    6,
                    install_name.is_none_or(|install| is_under(install, &SYSTEM_INSTALL_PREFIXES)),
                    || format!("system {label} needs an install_name under /usr/lib/ or /System/Library/"),
                ),
                Some(other) => v.push(
                    6,
                    format!("{label} kind must be `bundled`, `own`, or `system`, found `{other}`"),
                ),
                None => v.push(6, format!("{label} needs kind")),
            }
        }
    }
}

/// A `gpg` signature needs an https URL, a key file in `runtimes/keys/`,
/// and the key's fingerprint.
fn check_gpg(root: &Path, signature: &UpstreamSignature, v: &mut Rules<'_>) {
    const WHEN: &str = "a gpg upstream_signature";
    v.require_https(5, "url", &signature.url, WHEN);
    match filled(&signature.key) {
        None => v.push(5, format!("{WHEN} needs key")),
        Some(key) => v.check(5, is_key_file(root, key), || {
            format!("key `{key}` must name a file in {KEYS_DIR}/")
        }),
    }
    v.check(
        5,
        signature
            .fingerprint
            .as_deref()
            .is_some_and(|fingerprint| normalize_fingerprint(fingerprint).is_some()),
        || format!("{WHEN} needs fingerprint as 40 or 64 hex characters"),
    );
}

/// True when `key` is `runtimes/keys/<file>` and that file exists.
fn is_key_file(root: &Path, key: &str) -> bool {
    let Some(file) = key
        .strip_prefix(KEYS_DIR)
        .and_then(|rest| rest.strip_prefix('/'))
    else {
        return false;
    };
    !file.is_empty()
        && !file.contains(['/', '\\'])
        && file != ".."
        && fs::symlink_metadata(root.join(KEYS_DIR).join(file)).is_ok_and(|meta| meta.is_file())
}

/// A GPG fingerprint in upper case without spaces, when it is 40 (v4) or 64
/// (v5) hex characters.
pub fn normalize_fingerprint(fingerprint: &str) -> Option<String> {
    let compact: String = fingerprint
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_ascii_uppercase();
    ((compact.len() == 40 || compact.len() == 64) && compact.bytes().all(|b| b.is_ascii_hexdigit()))
        .then_some(compact)
}

/// The value when it is present and not blank.
fn filled(value: &Option<String>) -> Option<&str> {
    value.as_deref().filter(|value| !value.trim().is_empty())
}

/// True for `https://` URLs with something after the scheme.
pub fn is_https(url: &str) -> bool {
    url.strip_prefix("https://")
        .is_some_and(|rest| !rest.is_empty() && !rest.contains(char::is_whitespace))
}

/// True when `path` starts with one of `prefixes` and never climbs back
/// out with a `..` part.
fn is_under(path: &str, prefixes: &[&str]) -> bool {
    prefixes.iter().any(|prefix| path.starts_with(prefix))
        && !path.split('/').any(|part| part == "..")
}

/// True for a non empty relative path with no `..`, `.`, or empty part.
pub fn is_inner_relative_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains('\\')
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != ".." && part != ".")
}

/// `a`, `a or b`, or `a, b, or c`, each in backticks.
fn or_list(values: &[&str]) -> String {
    let quoted: Vec<String> = values.iter().map(|value| format!("`{value}`")).collect();
    match quoted.as_slice() {
        [] => String::new(),
        [one] => one.clone(),
        [first, second] => format!("{first} or {second}"),
        [rest @ .., last] => format!("{}, or {last}", rest.join(", ")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const UPSTREAM: &str = r#"
schema_version = 1

[[artifact]]
id = "nats-2.15.1-macos-aarch64"
provider = "nats"
line = "2.15"
version = "2.15.1"
os = "macos"
arch = "aarch64"
min_os = "11.0"
status = "available"
origin = "upstream"
distribution = "upstream_download"
format = "tar_gz"
urls = ["https://github.com/nats-io/nats-server/releases/download/v2.15.1/nats-server-v2.15.1-darwin-arm64.tar.gz"]
sha256 = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
size = 7340032
archive_root = "nats-server-v2.15.1-darwin-arm64"
upstream_checksum = { kind = "sha256sums", url = "https://github.com/nats-io/nats-server/releases/download/v2.15.1/SHA256SUMS" }
upstream_signature = { kind = "none" }
signing_policy = "keep_or_adhoc"
license = "Apache-2.0"
license_url = "https://github.com/nats-io/nats-server/blob/v2.15.1/LICENSE"
executables = ["nats-server"]

[[artifact.dependency]]
name = "libSystem"
kind = "system"
install_name = "/usr/lib/libSystem.B.dylib"
"#;

    const MAINTAINER: &str = r#"
schema_version = 1

[[artifact]]
id = "postgresql-18.6-r1-macos-aarch64"
provider = "postgresql"
line = "18"
version = "18.6"
build = 1
os = "macos"
arch = "aarch64"
min_os = "26.0"
status = "available"
origin = "maintainer"
distribution = "stackly_hosted"
format = "tar_gz"
urls = ["https://github.com/ojiepermana/stackly-runtimes/releases/download/postgresql-18.6-r1/postgresql-18.6-r1-macos-aarch64.tar.gz"]
sha256 = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
size = 52428800
archive_root = "postgresql-18.6-r1-macos-aarch64"
source_url = "https://ftp.postgresql.org/pub/source/v18.6/postgresql-18.6.tar.bz2"
source_sha256 = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
recipe = "ojiepermana/stackly-runtimes@0123456789abcdef0123456789abcdef01234567:recipes/postgresql.sh"
attestation = "https://github.com/ojiepermana/stackly-runtimes/attestations/1"
signing_policy = "adhoc_pipeline"
license = "PostgreSQL"
license_url = "https://www.postgresql.org/about/licence/"
executables = ["bin/postgres", "bin/initdb", "bin/psql"]

[[artifact.dependency]]
name = "libssl.3.dylib"
kind = "bundled"
install_name = "@rpath/libssl.3.dylib"
version = "3.5.4"
license = "Apache-2.0"
source_url = "https://github.com/openssl/openssl/releases/download/openssl-3.5.4/openssl-3.5.4.tar.gz"

[[artifact.dependency]]
name = "libpq.5.dylib"
kind = "own"
install_name = "@rpath/libpq.5.dylib"

[[artifact.dependency]]
name = "libz"
kind = "system"
install_name = "/usr/lib/libz.1.dylib"
"#;

    const BLOCKED: &str = r#"
schema_version = 1

[[artifact]]
id = "clickhouse-26.9.1.1-macos-aarch64"
provider = "clickhouse"
line = "26.9"
version = "26.9.1.1"
os = "macos"
arch = "aarch64"
status = "blocked"
blocked_reason = "no release carries a clickhouse-macos-aarch64 asset"
blocked_owner = "@ojiepermana"
blocked_since = 2026-10-17
origin = "upstream"
distribution = "upstream_download"
signing_policy = "keep_or_adhoc"
upstream_checksum = { kind = "none" }
upstream_signature = { kind = "none" }
"#;

    /// The rule 1 to 6 lines of `text`, checked against an empty root.
    fn check(text: &str) -> Vec<String> {
        let root = tempfile::tempdir().unwrap();
        check_in(root.path(), text)
    }

    fn check_in(root: &Path, text: &str) -> Vec<String> {
        Inventory::parse(text)
            .unwrap()
            .violations(root)
            .iter()
            .map(ToString::to_string)
            .collect()
    }

    /// `text` with `from` replaced by `to`; fails when `from` is absent, so
    /// a fixture edit never silently tests nothing.
    fn edit(text: &str, from: &str, to: &str) -> String {
        assert!(text.contains(from), "fixture has no `{from}`");
        text.replacen(from, to, 1)
    }

    fn assert_one(lines: &[String], expected: &str) {
        assert_eq!(lines, [expected.to_owned()], "violations: {lines:#?}");
    }

    #[test]
    fn an_empty_inventory_with_schema_version_1_is_valid() {
        assert!(check("schema_version = 1\n").is_empty());
    }

    #[test]
    fn valid_upstream_maintainer_and_blocked_artifacts_pass() {
        assert_eq!(check(UPSTREAM), Vec::<String>::new());
        assert_eq!(check(MAINTAINER), Vec::<String>::new());
        assert_eq!(check(BLOCKED), Vec::<String>::new());
    }

    #[test]
    fn unknown_keys_and_bad_toml_are_refused() {
        assert!(Inventory::parse(&edit(UPSTREAM, "min_os", "minimum_os")).is_err());
        assert!(Inventory::parse("schema_version = ").is_err());
    }

    #[test]
    fn rule_1_needs_schema_version_1() {
        assert_one(
            &check("schema_version = 2\n"),
            "inventory: schema_version must be 1 (rule 1)",
        );
        assert_one(&check(""), "inventory: schema_version must be 1 (rule 1)");
    }

    #[test]
    fn rule_1_derives_the_id_from_provider_version_build_os_and_arch() {
        assert_one(
            &check(&edit(
                UPSTREAM,
                "id = \"nats-2.15.1-macos-aarch64\"",
                "id = \"nats-2.15.1\"",
            )),
            "nats-2.15.1: id must be `nats-2.15.1-macos-aarch64` (rule 1)",
        );
        assert_one(
            &check(&edit(MAINTAINER, "build = 1", "build = 2")),
            "postgresql-18.6-r1-macos-aarch64: id must be `postgresql-18.6-r2-macos-aarch64` (rule 1)",
        );
    }

    #[test]
    fn rule_1_needs_an_id_and_a_known_provider() {
        let lines = check(&edit(UPSTREAM, "id = \"nats-2.15.1-macos-aarch64\"\n", ""));
        assert_one(&lines, "artifact #1: every artifact needs id (rule 1)");
        let lines = check(&edit(
            UPSTREAM,
            "provider = \"nats\"",
            "provider = \"redis\"",
        ));
        assert!(lines.contains(
            &"nats-2.15.1-macos-aarch64: provider must be `postgresql`, `mysql`, `nats`, `clickhouse`, `valkey`, or `traefik`, found `redis` (rule 1)"
                .to_owned()
        ));
    }

    #[test]
    fn rule_1_refuses_a_repeated_id_and_a_repeated_line() {
        let artifact = UPSTREAM.split_once("[[artifact]]").unwrap().1;
        let twice = format!("{UPSTREAM}\n[[artifact]]{artifact}");
        let lines = check(&twice);
        assert_eq!(
            lines,
            [
                "nats-2.15.1-macos-aarch64: id is used by more than one artifact (rule 1)",
                "nats-2.15.1-macos-aarch64: provider, line, os, and arch repeat those of nats-2.15.1-macos-aarch64 (rule 1)",
            ]
        );
    }

    #[test]
    fn rule_2_needs_an_exact_version_inside_the_line() {
        assert_one(
            &check(&edit(UPSTREAM, "line = \"2.15\"", "line = \"2.14\"")),
            "nats-2.15.1-macos-aarch64: version `2.15.1` must start with `2.14.` (rule 2)",
        );
        let selector = |version: &str| {
            check(&edit(
                &edit(
                    UPSTREAM,
                    "version = \"2.15.1\"",
                    &format!("version = \"{version}\""),
                ),
                "id = \"nats-2.15.1-macos-aarch64\"",
                &format!("id = \"nats-{version}-macos-aarch64\""),
            ))
        };
        assert_one(
            &selector("2.15.latest"),
            "nats-2.15.latest-macos-aarch64: version `2.15.latest` is not exact (rule 2)",
        );
        assert_one(
            &selector("2.15"),
            "nats-2.15-macos-aarch64: version `2.15` must start with `2.15.` (rule 2)",
        );
    }

    #[test]
    fn rule_2_needs_macos_and_aarch64() {
        let lines = check(&edit(
            &edit(UPSTREAM, "arch = \"aarch64\"", "arch = \"x86_64\""),
            "id = \"nats-2.15.1-macos-aarch64\"",
            "id = \"nats-2.15.1-macos-x86_64\"",
        ));
        assert_one(
            &lines,
            "nats-2.15.1-macos-x86_64: every artifact needs arch = \"aarch64\" (rule 2)",
        );
    }

    #[test]
    fn rule_3_needs_every_field_of_an_available_artifact() {
        for field in [
            "min_os = \"11.0\"\n",
            "license = \"Apache-2.0\"\n",
            "executables = [\"nats-server\"]\n",
        ] {
            let lines = check(&edit(UPSTREAM, field, ""));
            assert_eq!(lines.len(), 1, "{field}: {lines:#?}");
            assert!(lines[0].ends_with("(rule 3)"), "{lines:#?}");
        }
        assert_one(
            &check(&edit(UPSTREAM, "size = 7340032", "size = 0")),
            "nats-2.15.1-macos-aarch64: an available artifact needs size greater than 0 (rule 3)",
        );
        assert_one(
            &check(&edit(UPSTREAM, "sha256 = \"aaaa", "sha256 = \"AAAA")),
            "nats-2.15.1-macos-aarch64: an available artifact needs sha256 as 64 lowercase hex characters (rule 3)",
        );
    }

    #[test]
    fn rule_3_needs_https_urls() {
        assert_one(
            &check(&edit(UPSTREAM, "urls = [\"https://", "urls = [\"http://")),
            "nats-2.15.1-macos-aarch64: urls must all be https:// URLs, found `http://github.com/nats-io/nats-server/releases/download/v2.15.1/nats-server-v2.15.1-darwin-arm64.tar.gz` (rule 3)",
        );
        assert_one(
            &check(&edit(
                UPSTREAM,
                "urls = [\"https://github.com/nats-io/nats-server/releases/download/v2.15.1/nats-server-v2.15.1-darwin-arm64.tar.gz\"]",
                "urls = []",
            )),
            "nats-2.15.1-macos-aarch64: an available artifact needs at least one URL in urls (rule 3)",
        );
    }

    #[test]
    fn rule_3_needs_executables_inside_the_artifact() {
        assert_one(
            &check(&edit(
                UPSTREAM,
                "executables = [\"nats-server\"]",
                "executables = [\"../nats-server\"]",
            )),
            "nats-2.15.1-macos-aarch64: executable `../nats-server` must be a plain relative path, without `..`, `.`, or empty parts (rule 3)",
        );
        assert_eq!(
            check(&edit(
                UPSTREAM,
                "executables = [\"nats-server\"]",
                "executables = [\"/usr/bin/true\"]"
            ))
            .len(),
            1
        );
    }

    #[test]
    fn rule_3_needs_a_known_status_format_and_signing_policy() {
        assert_one(
            &check(&edit(
                UPSTREAM,
                "status = \"available\"",
                "status = \"ready\"",
            )),
            "nats-2.15.1-macos-aarch64: status must be `available` or `blocked`, found `ready` (rule 3)",
        );
        assert_one(
            &check(&edit(UPSTREAM, "format = \"tar_gz\"", "format = \"zip\"")),
            "nats-2.15.1-macos-aarch64: format must be `tar_gz` or `binary`, found `zip` (rule 3)",
        );
    }

    #[test]
    fn rule_3_needs_format_license_url_and_signing_policy() {
        let without_format = check(&edit(UPSTREAM, "format = \"tar_gz\"\n", ""));
        assert_one(
            &without_format,
            "nats-2.15.1-macos-aarch64: an available artifact needs format (rule 3)",
        );
        assert_one(
            &check(&edit(
                UPSTREAM,
                "license_url = \"https://github.com/nats-io/nats-server/blob/v2.15.1/LICENSE\"",
                "license_url = \"http://github.com/nats-io/nats-server/blob/v2.15.1/LICENSE\"",
            )),
            "nats-2.15.1-macos-aarch64: license_url must be an https:// URL, found `http://github.com/nats-io/nats-server/blob/v2.15.1/LICENSE` (rule 3)",
        );
        let unknown_policy = check(&edit(
            UPSTREAM,
            "signing_policy = \"keep_or_adhoc\"",
            "signing_policy = \"developer_id\"",
        ));
        assert_eq!(
            unknown_policy,
            [
                "nats-2.15.1-macos-aarch64: signing_policy must be `keep_or_adhoc` or `adhoc_pipeline`, found `developer_id` (rule 3)",
                "nats-2.15.1-macos-aarch64: an upstream artifact needs signing_policy = \"keep_or_adhoc\" (rule 5)",
            ]
        );
    }

    #[test]
    fn rule_4_needs_reason_owner_and_date_for_a_blocked_artifact() {
        assert_one(
            &check(&edit(
                BLOCKED,
                "blocked_owner = \"@ojiepermana\"",
                "blocked_owner = \"ojiepermana\"",
            )),
            "clickhouse-26.9.1.1-macos-aarch64: a blocked artifact needs blocked_owner as an @handle (rule 4)",
        );
        assert_one(
            &check(&edit(BLOCKED, "blocked_owner = \"@ojiepermana\"\n", "")),
            "clickhouse-26.9.1.1-macos-aarch64: a blocked artifact needs blocked_owner as an @handle (rule 4)",
        );
        assert_one(
            &check(&edit(
                BLOCKED,
                "blocked_reason = \"no release carries a clickhouse-macos-aarch64 asset\"\n",
                "",
            )),
            "clickhouse-26.9.1.1-macos-aarch64: a blocked artifact needs blocked_reason (rule 4)",
        );
        assert_one(
            &check(&edit(
                BLOCKED,
                "blocked_since = 2026-10-17",
                "blocked_since = 2026-10-17T10:00:00Z",
            )),
            "clickhouse-26.9.1.1-macos-aarch64: a blocked artifact needs blocked_since as a date, for example 2026-10-17 (rule 4)",
        );
    }

    #[test]
    fn rule_5_needs_the_maintainer_fields() {
        assert_one(
            &check(&edit(
                MAINTAINER,
                "attestation = \"https://github.com/ojiepermana/stackly-runtimes/attestations/1\"\n",
                "",
            )),
            "postgresql-18.6-r1-macos-aarch64: a maintainer artifact needs attestation (rule 5)",
        );
        assert_one(
            &check(&edit(
                MAINTAINER,
                "signing_policy = \"adhoc_pipeline\"",
                "signing_policy = \"keep_or_adhoc\"",
            )),
            "postgresql-18.6-r1-macos-aarch64: a maintainer artifact needs signing_policy = \"adhoc_pipeline\" (rule 5)",
        );
        let lines = check(&edit(
            &edit(MAINTAINER, "build = 1\n", ""),
            "id = \"postgresql-18.6-r1-macos-aarch64\"",
            "id = \"postgresql-18.6-macos-aarch64\"",
        ));
        assert_one(
            &lines,
            "postgresql-18.6-macos-aarch64: a maintainer artifact needs build of 1 or more (rule 5)",
        );
    }

    #[test]
    fn rule_5_needs_the_upstream_fields() {
        let lines = check(&edit(
            &edit(
                UPSTREAM,
                "signing_policy = \"keep_or_adhoc\"\n",
                "build = 1\nsigning_policy = \"keep_or_adhoc\"\n",
            ),
            "id = \"nats-2.15.1-macos-aarch64\"",
            "id = \"nats-2.15.1-r1-macos-aarch64\"",
        ));
        assert_one(
            &lines,
            "nats-2.15.1-r1-macos-aarch64: an upstream artifact has no build (rule 5)",
        );
        assert_one(
            &check(&edit(
                UPSTREAM,
                "upstream_signature = { kind = \"none\" }\n",
                "",
            )),
            "nats-2.15.1-macos-aarch64: an upstream artifact needs upstream_signature (rule 5)",
        );
        assert_one(
            &check(&edit(
                UPSTREAM,
                "upstream_checksum = { kind = \"sha256sums\", url = \"https://github.com/nats-io/nats-server/releases/download/v2.15.1/SHA256SUMS\" }",
                "upstream_checksum = { kind = \"sha256sums\" }",
            )),
            "nats-2.15.1-macos-aarch64: an upstream checksum needs upstream_checksum.url (rule 5)",
        );
    }

    #[test]
    fn rule_5_needs_a_gpg_key_file_in_runtimes_keys() {
        let gpg = edit(
            UPSTREAM,
            "upstream_signature = { kind = \"none\" }",
            "upstream_signature = { kind = \"gpg\", url = \"https://example.org/a.asc\", key = \"runtimes/keys/nats.asc\", fingerprint = \"A4A9 4068 76FC BD3C 4567  70C8 8C71 8D3B 5072 E1F5\" }",
        );
        let root = tempfile::tempdir().unwrap();
        assert_one(
            &check_in(root.path(), &gpg),
            "nats-2.15.1-macos-aarch64: key `runtimes/keys/nats.asc` must name a file in runtimes/keys/ (rule 5)",
        );
        fs::create_dir_all(root.path().join(KEYS_DIR)).unwrap();
        fs::write(root.path().join(KEYS_DIR).join("nats.asc"), "key").unwrap();
        assert!(check_in(root.path(), &gpg).is_empty());

        let escaping = edit(
            &gpg,
            "runtimes/keys/nats.asc",
            "runtimes/keys/../inventory.toml",
        );
        assert_eq!(check_in(root.path(), &escaping).len(), 1);
        let short = edit(&gpg, "A4A9 4068", "A4A9");
        assert_one(
            &check_in(root.path(), &short),
            "nats-2.15.1-macos-aarch64: a gpg upstream_signature needs fingerprint as 40 or 64 hex characters (rule 5)",
        );
    }

    #[test]
    fn rule_6_needs_version_license_and_source_for_a_bundled_dependency() {
        assert_one(
            &check(&edit(
                MAINTAINER,
                "license = \"Apache-2.0\"\nsource_url",
                "source_url",
            )),
            "postgresql-18.6-r1-macos-aarch64: bundled dependency libssl.3.dylib needs license (rule 6)",
        );
        assert_one(
            &check(&edit(
                MAINTAINER,
                "install_name = \"@rpath/libssl.3.dylib\"",
                "install_name = \"/opt/homebrew/lib/libssl.3.dylib\"",
            )),
            "postgresql-18.6-r1-macos-aarch64: bundled dependency libssl.3.dylib needs an install_name starting with @rpath/, @loader_path/, or @executable_path/ (rule 6)",
        );
    }

    #[test]
    fn rule_6_matches_install_names_to_the_kind() {
        assert_one(
            &check(&edit(
                MAINTAINER,
                "install_name = \"@rpath/libpq.5.dylib\"",
                "install_name = \"/usr/local/lib/libpq.5.dylib\"",
            )),
            "postgresql-18.6-r1-macos-aarch64: own dependency libpq.5.dylib needs an install_name starting with @rpath/, @loader_path/, or @executable_path/ (rule 6)",
        );
        assert_one(
            &check(&edit(
                UPSTREAM,
                "install_name = \"/usr/lib/libSystem.B.dylib\"",
                "install_name = \"/opt/homebrew/lib/libSystem.B.dylib\"",
            )),
            "nats-2.15.1-macos-aarch64: system dependency libSystem needs an install_name under /usr/lib/ or /System/Library/ (rule 6)",
        );
        assert_one(
            &check(&edit(UPSTREAM, "kind = \"system\"", "kind = \"vendored\"")),
            "nats-2.15.1-macos-aarch64: dependency libSystem kind must be `bundled`, `own`, or `system`, found `vendored` (rule 6)",
        );
    }

    #[test]
    fn rule_6_refuses_an_install_name_that_climbs_out_with_dot_dot() {
        assert_one(
            &check(&edit(
                UPSTREAM,
                "install_name = \"/usr/lib/libSystem.B.dylib\"",
                "install_name = \"/usr/lib/../../opt/homebrew/lib/libSystem.B.dylib\"",
            )),
            "nats-2.15.1-macos-aarch64: system dependency libSystem needs an install_name under /usr/lib/ or /System/Library/ (rule 6)",
        );
        assert_eq!(
            check(&edit(
                MAINTAINER,
                "install_name = \"@rpath/libpq.5.dylib\"",
                "install_name = \"@rpath/../../opt/lib/libpq.5.dylib\"",
            ))
            .len(),
            1
        );
    }

    #[test]
    fn rule_6_refuses_a_repeated_dependency_name() {
        let twice = format!(
            "{UPSTREAM}\n[[artifact.dependency]]\nname = \"libSystem\"\nkind = \"system\"\ninstall_name = \"/usr/lib/libSystem.B.dylib\"\n"
        );
        assert_one(
            &check(&twice),
            "nats-2.15.1-macos-aarch64: dependency libSystem is listed more than once (rule 6)",
        );
    }

    #[test]
    fn fingerprints_are_normalized_to_upper_case_without_spaces() {
        assert_eq!(
            normalize_fingerprint("a4a9 406876fcbd3c456770c88c718d3b5072e1f5").as_deref(),
            Some("A4A9406876FCBD3C456770C88C718D3B5072E1F5")
        );
        assert_eq!(normalize_fingerprint("A4A9"), None);
        assert_eq!(normalize_fingerprint(&"g".repeat(40)), None);
    }
}
