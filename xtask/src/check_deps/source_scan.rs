//! The source scan of `cargo xtask check-deps`. Five rule sets:
//!
//! - Library rules (spec 0004, "Peta modul dan dependency"): library crates
//!   never read environment variables and never hardcode a machine path.
//!   Bins read `STACKLY_ROOT` once in `main`, and every other path comes
//!   from `DirectoryLayout`.
//! - Socket rule (spec 0006, AC-5): the transport, the daemon, and the CLI
//!   never name `TcpListener`, `TcpStream`, `TcpSocket`, or `UdpSocket`, as
//!   a path or any other identifier, so no TCP or UDP socket is opened.
//! - Expose rule (spec 0011, AC-15), for every crate: a secret value is read
//!   only through `expose`, so a call to a method named `expose` (`.expose`,
//!   or a path ending in `::expose`) is allowed only in the modules of
//!   [`EXPOSE_ALLOWED`]. Attribute arguments are read for this rule too, so
//!   a call inside `#[instrument(fields(...))]` counts.
//! - CLI rules (spec 0008, AC-11 and BR-011), for `stackly-cli` only: no
//!   `std::process` item other than `exit`, `ExitCode`, and `id` (a `use`
//!   of the module itself, renamed or not, and a glob count too), no
//!   `tokio::process`, no `std::os::unix::process`, and no `print!`,
//!   `println!`, `eprint!`, or `eprintln!`, which panic on a broken pipe.
//! - CLI code rule (spec 0008, AC-14), for every crate but `stackly-cli`
//!   and `stackly-protocol`: no path names `codes::cli`, so the daemon never
//!   sends a code of the `cli` area.
//!
//! Each crate is read along its module tree, starting at its lib and bin
//! targets, so `tests/` and a file only a `#[cfg(test)] mod` loads are never
//! scanned. Items marked `#[cfg(test)]` (alone or inside `all(...)`) or
//! `#[test]` are skipped, and so are attributes, doc comments included.
//! Macro bodies are read token by token, so a path inside `format!` counts.
//!
//! An environment read is a path ending in `env::<name>` (`std::env::var`,
//! `env::var_os`), or a `use` of such a name or of `env::*`. A function of
//! the same name elsewhere, such as the macOS adapter's own `home_dir`, is
//! not one. Compile time `env!` reads the build environment and is allowed.
//! A socket name inside a string or a doc comment is not an identifier and
//! passes.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use cargo_metadata::Package;
use proc_macro2::{Spacing, TokenStream, TokenTree};
use syn::parse::Parser;
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::visit::{self, Visit};
use syn::{Attribute, Expr, ExprLit, ImplItem, Item, Lit, Macro, Meta, Token, UseTree};

/// The crates whose source the library rules scan.
pub const SCANNED: [&str; 5] = [
    "stackly-domain",
    "stackly-platform",
    "stackly-core",
    "stackly-protocol",
    "stackly-platform-macos",
];
/// The crates whose source the socket rule scans.
pub const SOCKET_SCANNED: [&str; 3] = ["stackly-ipc", "stacklyd", "stackly-cli"];
/// The identifiers the socket rule refuses.
const SOCKET_NAMES: [&str; 4] = ["TcpListener", "TcpStream", "TcpSocket", "UdpSocket"];
/// The socket rule's message.
const SOCKET_RULE: &str = "no tcp or udp sockets (spec 0006)";
/// The expose rule's message.
const EXPOSE_RULE: &str = "secret expose outside the allowlist";
/// The crate the CLI rules scan.
const CLI_CRATE: &str = "stackly-cli";
/// The crates that may name `codes::cli`.
const CLI_CODES_ALLOWED: [&str; 2] = [CLI_CRATE, "stackly-protocol"];
/// The CLI spawn rule's message.
pub const SPAWN_RULE: &str = "the cli never spawns processes (spec 0008, BR-011)";
/// The `std::process` items the CLI may name.
const PROCESS_ALLOWED: [&str; 3] = ["exit", "ExitCode", "id"];
/// The print macros the CLI never calls.
const PRINT_MACROS: [&str; 4] = ["print", "println", "eprint", "eprintln"];
/// The CLI print rule's message.
const PRINT_RULE: &str = "the cli writes through its output module, never print macros, which panic on a broken pipe (spec 0008, AC-10)";
/// The CLI code rule's message.
const CLI_CODES_RULE: &str = "only the cli and the protocol name codes::cli, so the daemon never sends a cli code (spec 0008, AC-14)";
/// The modules that may call `expose` on a secret value (spec 0011,
/// *Penjaga di kode*), each with its submodules: exactly the call sites
/// that exist. A crate of the spec's table adds its module here once it
/// has a call site (`stackly-platform` `secret`,
/// `stacklyd` `ipc::secret`, `stackly-cli` `secret_input` and
/// `commands::secret`).
pub const EXPOSE_ALLOWED: [(&str, &[&str]); 4] = [
    ("stackly-core", &["secret::redact", "secret::delivery"]),
    ("stackly-platform-macos", &["secret", "process::stdin"]),
    ("stackly-protocol", &["secret"]),
    ("stackly-testkit", &["fake_secret"]),
];
/// The `std::env` functions a library never calls.
const ENV_READS: [&str; 6] = ["var", "var_os", "vars", "vars_os", "home_dir", "temp_dir"];
/// Text a string literal in a library never holds.
const MACHINE_PATHS: [&str; 5] = [
    "/Users/",
    "/opt/homebrew",
    "/usr/local",
    "/tmp",
    "/var/folders",
];

/// Which rule sets apply to a crate. The expose rule applies to every
/// crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rules {
    /// The library rules: no environment reads, no machine paths.
    pub library: bool,
    /// The socket rule: no TCP or UDP socket names.
    pub sockets: bool,
    /// The modules of this crate that may call `expose`, as `a::b` paths.
    pub expose_allowed: &'static [&'static str],
    /// The CLI rules: no process spawning paths, no print macros.
    pub cli: bool,
    /// The CLI code rule: no path naming `codes::cli`.
    pub no_cli_codes: bool,
}

impl Rules {
    /// The rule sets for the crate called `name`.
    #[must_use]
    pub fn for_crate(name: &str) -> Self {
        Self {
            library: SCANNED.contains(&name),
            sockets: SOCKET_SCANNED.contains(&name),
            expose_allowed: EXPOSE_ALLOWED
                .iter()
                .find(|(crate_name, _)| *crate_name == name)
                .map_or(&[], |(_, modules)| *modules),
            cli: name == CLI_CRATE,
            no_cli_codes: !CLI_CODES_ALLOWED.contains(&name),
        }
    }
}

/// Scans the lib and bin targets of `package` with `rules`. Messages read
/// `<crate> <file>:<line>: <rule>`, with `file` relative to `root`.
///
/// # Errors
///
/// Returns an error when a source file cannot be read or parsed, or a
/// `mod` declaration names a file that does not exist.
pub fn scan_package(root: &Path, package: &Package, rules: Rules) -> Result<Vec<String>> {
    let entries: Vec<PathBuf> = package
        .targets
        .iter()
        .filter(|target| target.is_lib() || target.is_bin())
        .map(|target| target.src_path.clone().into_std_path_buf())
        .collect();
    scan_crate(root, package.name.as_str(), &entries, rules)
}

/// Scans one crate from its root source files.
fn scan_crate(root: &Path, name: &str, entries: &[PathBuf], rules: Rules) -> Result<Vec<String>> {
    let mut pending: Vec<(PathBuf, PathBuf, Vec<String>)> = entries
        .iter()
        .map(|entry| (entry.clone(), parent(entry), Vec::new()))
        .collect();
    let mut seen: Vec<PathBuf> = Vec::new();
    let mut violations = Vec::new();
    while let Some((file, module_dir, module_path)) = pending.pop() {
        if seen.contains(&file) {
            continue;
        }
        seen.push(file.clone());
        let source =
            fs::read_to_string(&file).with_context(|| format!("cannot read {}", file.display()))?;
        let syntax = syn::parse_file(&source)
            .with_context(|| format!("cannot parse {} as Rust", file.display()))?;
        let mut scan = Scan {
            rules,
            file_dir: parent(&file),
            module_dir,
            module_path,
            inline: Vec::new(),
            findings: Vec::new(),
            modules: Vec::new(),
        };
        scan.visit_file(&syntax);
        let shown = file
            .strip_prefix(root)
            .unwrap_or(&file)
            .display()
            .to_string();
        for (line, rule) in scan.findings {
            violations.push(format!("{name} {shown}:{line}: {rule}"));
        }
        for module in scan.modules {
            pending.push(module.resolve()?);
        }
    }
    Ok(violations)
}

fn parent(file: &Path) -> PathBuf {
    file.parent().map(Path::to_path_buf).unwrap_or_default()
}

/// A `mod name;` whose file is still to be found.
struct OutOfLine {
    /// The folder the module's file sits in.
    dir: PathBuf,
    name: String,
    /// The `#[path]` value, joined to the right folder already.
    path: Option<PathBuf>,
    /// The module's path from the crate root, its own name last.
    module_path: Vec<String>,
}

impl OutOfLine {
    /// Returns the module's file, the folder its own child modules sit in,
    /// and its module path.
    fn resolve(self) -> Result<(PathBuf, PathBuf, Vec<String>)> {
        if let Some(path) = self.path {
            // A file loaded through `#[path]` keeps its children beside it.
            let children = parent(&path);
            return Ok((path, children, self.module_path));
        }
        let flat = self.dir.join(format!("{}.rs", self.name));
        if flat.is_file() {
            let children = self.dir.join(&self.name);
            return Ok((flat, children, self.module_path));
        }
        let nested = self.dir.join(&self.name).join("mod.rs");
        if nested.is_file() {
            let children = self.dir.join(&self.name);
            return Ok((nested, children, self.module_path));
        }
        anyhow::bail!(
            "cannot find the file of `mod {};` in {}",
            self.name,
            self.dir.display()
        )
    }
}

struct Scan {
    rules: Rules,
    /// The folder of the file being scanned.
    file_dir: PathBuf,
    /// The folder the file's own child modules sit in.
    module_dir: PathBuf,
    /// The file's module path from the crate root.
    module_path: Vec<String>,
    /// Inline `mod name { ... }` blocks around the current node.
    inline: Vec<String>,
    findings: Vec<(usize, String)>,
    modules: Vec<OutOfLine>,
}

impl Scan {
    fn report(&mut self, span: proc_macro2::Span, rule: String) {
        self.findings.push((span.start().line, rule));
    }

    fn env_read(&mut self, span: proc_macro2::Span, name: &str) {
        if !self.rules.library {
            return;
        }
        self.report(
            span,
            format!(
                "library crates never read the environment (`env::{name}`); a bin reads it once in main and passes the value in"
            ),
        );
    }

    fn check_text(&mut self, span: proc_macro2::Span, text: &str) {
        if !self.rules.library {
            return;
        }
        if let Some(path) = MACHINE_PATHS.iter().find(|path| text.contains(*path)) {
            self.report(
                span,
                format!(
                    "library crates never hardcode a machine path (`{path}`); derive it from DirectoryLayout"
                ),
            );
        }
    }

    fn check_lit(&mut self, lit: &Lit) {
        match lit {
            Lit::Str(text) => self.check_text(text.span(), &text.value()),
            Lit::ByteStr(bytes) => {
                self.check_text(bytes.span(), &String::from_utf8_lossy(&bytes.value()));
            }
            Lit::CStr(text) => {
                self.check_text(text.span(), &text.value().to_string_lossy());
            }
            _ => {}
        }
    }

    /// Reports a TCP or UDP socket name, once per line.
    fn check_ident(&mut self, ident: &proc_macro2::Ident) {
        if !self.rules.sockets || !SOCKET_NAMES.iter().any(|socket| ident == socket) {
            return;
        }
        let line = ident.span().start().line;
        if !self
            .findings
            .iter()
            .any(|(seen, rule)| *seen == line && rule == SOCKET_RULE)
        {
            self.findings.push((line, SOCKET_RULE.to_owned()));
        }
    }

    /// Reports a call to `expose` outside the allowed modules, once per
    /// line.
    fn expose_call(&mut self, span: proc_macro2::Span) {
        let current: Vec<&str> = self
            .module_path
            .iter()
            .chain(&self.inline)
            .map(String::as_str)
            .collect();
        let allowed = self.rules.expose_allowed.iter().any(|module| {
            let module: Vec<&str> = module.split("::").collect();
            current.starts_with(&module)
        });
        let line = span.start().line;
        if !allowed
            && !self
                .findings
                .iter()
                .any(|(seen, rule)| *seen == line && rule == EXPOSE_RULE)
        {
            self.findings.push((line, EXPOSE_RULE.to_owned()));
        }
    }

    /// Reads tokens for `. expose` and `:: expose`, as a macro body or an
    /// attribute holds them.
    fn check_expose_tokens(&mut self, tokens: TokenStream) {
        let tokens: Vec<TokenTree> = tokens.into_iter().collect();
        for (index, token) in tokens.iter().enumerate() {
            match token {
                TokenTree::Group(group) => self.check_expose_tokens(group.stream()),
                TokenTree::Ident(ident) if ident == "expose" && index > 0 => {
                    let before = &tokens[..index];
                    let method = matches!(
                        before.last(),
                        Some(TokenTree::Punct(dot)) if dot.as_char() == '.'
                    );
                    let path = matches!(
                        before,
                        [.., TokenTree::Punct(first), TokenTree::Punct(second)]
                            if first.as_char() == ':'
                                && first.spacing() == Spacing::Joint
                                && second.as_char() == ':'
                    );
                    if method || path {
                        self.expose_call(ident.span());
                    }
                }
                _ => {}
            }
        }
    }

    /// Reads a macro body token by token: string literals, identifiers,
    /// and `env :: <name>` sequences.
    fn check_tokens(&mut self, tokens: TokenStream) {
        let tokens: Vec<TokenTree> = tokens.into_iter().collect();
        for (index, token) in tokens.iter().enumerate() {
            match token {
                TokenTree::Group(group) => self.check_tokens(group.stream()),
                TokenTree::Literal(literal) => self.check_lit(&Lit::new(literal.clone())),
                TokenTree::Ident(ident) if ident != "env" => self.check_ident(ident),
                TokenTree::Ident(_) => {
                    if let [
                        TokenTree::Punct(first),
                        TokenTree::Punct(second),
                        TokenTree::Ident(name),
                        ..,
                    ] = &tokens[index + 1..]
                        && first.as_char() == ':'
                        && first.spacing() == Spacing::Joint
                        && second.as_char() == ':'
                        && ENV_READS.iter().any(|read| name == read)
                    {
                        self.env_read(name.span(), &name.to_string());
                    }
                }
                _ => {}
            }
        }
    }

    /// Reports `use ...env::<name>` and `use ...env::*`.
    fn check_use(&mut self, tree: &UseTree, parent: Option<&syn::Ident>) {
        let under_env = parent.is_some_and(|parent| parent == "env");
        match tree {
            UseTree::Path(path) => self.check_use(&path.tree, Some(&path.ident)),
            UseTree::Group(group) => {
                for item in &group.items {
                    self.check_use(item, parent);
                }
            }
            UseTree::Name(name) if under_env && is_env_read(&name.ident) => {
                self.env_read(name.ident.span(), &name.ident.to_string());
            }
            UseTree::Rename(rename) if under_env && is_env_read(&rename.ident) => {
                self.env_read(rename.ident.span(), &rename.ident.to_string());
            }
            UseTree::Glob(glob) if under_env => self.env_read(glob.span(), "*"),
            _ => {}
        }
    }

    /// Reports `rule` at `span`, once per line.
    fn report_once(&mut self, span: proc_macro2::Span, rule: &str) {
        let line = span.start().line;
        if !self
            .findings
            .iter()
            .any(|(seen, found)| *seen == line && found == rule)
        {
            self.findings.push((line, rule.to_owned()));
        }
    }

    /// Checks one path, as its segment names, against the CLI rules.
    fn check_segments(&mut self, span: proc_macro2::Span, segments: &[String]) {
        if self.rules.cli && spawns(segments) {
            self.report_once(span, SPAWN_RULE);
        }
        if self.rules.no_cli_codes && names_cli_codes(segments) {
            self.report_once(span, CLI_CODES_RULE);
        }
    }

    /// Reports a print macro call by its name.
    fn check_macro_name(&mut self, ident: &proc_macro2::Ident) {
        if self.rules.cli && PRINT_MACROS.iter().any(|name| ident == name) {
            self.report_once(ident.span(), PRINT_RULE);
        }
    }

    /// Reads a macro body for paths (`a :: b :: c` runs of identifiers)
    /// and print macro calls (`println !`), for the CLI rules.
    fn check_token_paths(&mut self, tokens: TokenStream) {
        if !self.rules.cli && !self.rules.no_cli_codes {
            return;
        }
        let tokens: Vec<TokenTree> = tokens.into_iter().collect();
        let mut index = 0;
        while index < tokens.len() {
            match &tokens[index] {
                TokenTree::Group(group) => self.check_token_paths(group.stream()),
                TokenTree::Ident(first) => {
                    let mut segments = vec![first.to_string()];
                    let mut next = index + 1;
                    while let [
                        TokenTree::Punct(colon),
                        TokenTree::Punct(second),
                        TokenTree::Ident(ident),
                        ..,
                    ] = &tokens[next..]
                        && colon.as_char() == ':'
                        && colon.spacing() == Spacing::Joint
                        && second.as_char() == ':'
                    {
                        segments.push(ident.to_string());
                        next += 3;
                    }
                    if let Some(TokenTree::Punct(bang)) = tokens.get(next)
                        && bang.as_char() == '!'
                        && let Some(TokenTree::Ident(name)) = tokens.get(next - 1)
                    {
                        self.check_macro_name(name);
                    }
                    self.check_segments(first.span(), &segments);
                    index = next;
                    continue;
                }
                _ => {}
            }
            index += 1;
        }
    }

    /// Checks every path a `use` tree imports against the CLI rules.
    fn check_cli_use(&mut self, tree: &UseTree, prefix: &mut Vec<String>) {
        match tree {
            UseTree::Path(path) => {
                prefix.push(path.ident.to_string());
                self.check_cli_use(&path.tree, prefix);
                prefix.pop();
            }
            UseTree::Group(group) => {
                for item in &group.items {
                    self.check_cli_use(item, prefix);
                }
            }
            UseTree::Name(name) => {
                let full = imported(prefix, &name.ident);
                self.check_use_target(name.ident.span(), &full);
            }
            UseTree::Rename(rename) => {
                let full = imported(prefix, &rename.ident);
                self.check_use_target(rename.ident.span(), &full);
            }
            UseTree::Glob(glob) => {
                let mut full = prefix.clone();
                full.push("*".to_owned());
                self.check_use_target(glob.span(), &full);
            }
        }
    }

    /// Checks one imported path. Importing the `std::process` module itself
    /// or its glob would let a later path bypass the item check, so both
    /// count as spawning.
    fn check_use_target(&mut self, span: proc_macro2::Span, full: &[String]) {
        if self.rules.cli && (spawns(full) || imports_process_module(full)) {
            self.report_once(span, SPAWN_RULE);
        }
        if self.rules.no_cli_codes && names_cli_codes(full) {
            self.report_once(span, CLI_CODES_RULE);
        }
    }

    /// The module path of a `mod name;` at the current node.
    fn child_module_path(&self, name: &str) -> Vec<String> {
        self.module_path
            .iter()
            .chain(&self.inline)
            .cloned()
            .chain([name.to_owned()])
            .collect()
    }

    /// The folder a `mod name;` at the current node resolves in.
    fn child_dir(&self) -> PathBuf {
        self.inline
            .iter()
            .fold(self.module_dir.clone(), |dir, name| dir.join(name))
    }
}

impl<'ast> Visit<'ast> for Scan {
    fn visit_item(&mut self, node: &'ast Item) {
        if !item_attrs(node).is_some_and(is_test_only) {
            visit::visit_item(self, node);
        }
    }

    fn visit_impl_item(&mut self, node: &'ast ImplItem) {
        let attrs = match node {
            ImplItem::Const(item) => &item.attrs,
            ImplItem::Fn(item) => &item.attrs,
            ImplItem::Type(item) => &item.attrs,
            ImplItem::Macro(item) => &item.attrs,
            _ => return visit::visit_impl_item(self, node),
        };
        if !is_test_only(attrs) {
            visit::visit_impl_item(self, node);
        }
    }

    fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
        let name = node.ident.to_string();
        if node.content.is_some() {
            self.inline.push(name);
            visit::visit_item_mod(self, node);
            self.inline.pop();
            return;
        }
        let path = path_attr(&node.attrs).map(|path| {
            let base = if self.inline.is_empty() {
                self.file_dir.clone()
            } else {
                self.child_dir()
            };
            base.join(path)
        });
        let module_path = self.child_module_path(&name);
        self.modules.push(OutOfLine {
            dir: self.child_dir(),
            name,
            path,
            module_path,
        });
    }

    fn visit_attribute(&mut self, node: &'ast Attribute) {
        // Attributes are metadata, and doc comments may name any path, so
        // only the expose rule reads their arguments: `#[name(...)]`, and a
        // macro as in `#[doc = concat!(...)]`.
        match &node.meta {
            Meta::List(list) => self.check_expose_tokens(list.tokens.clone()),
            Meta::NameValue(name_value) => {
                if let Expr::Macro(value) = &name_value.value {
                    self.check_expose_tokens(value.mac.tokens.clone());
                }
            }
            Meta::Path(_) => {}
        }
    }

    fn visit_expr_method_call(&mut self, node: &'ast syn::ExprMethodCall) {
        if node.method == "expose" {
            self.expose_call(node.method.span());
        }
        visit::visit_expr_method_call(self, node);
    }

    fn visit_path(&mut self, node: &'ast syn::Path) {
        let segments: Vec<&syn::Ident> = node.segments.iter().map(|s| &s.ident).collect();
        if let [.., env, name] = segments.as_slice()
            && *env == "env"
            && is_env_read(name)
        {
            self.env_read(name.span(), &name.to_string());
        }
        if let [_, .., last] = segments.as_slice()
            && *last == "expose"
        {
            self.expose_call(last.span());
        }
        if let Some(first) = segments.first() {
            let names: Vec<String> = segments.iter().map(ToString::to_string).collect();
            self.check_segments(first.span(), &names);
        }
        visit::visit_path(self, node);
    }

    fn visit_item_use(&mut self, node: &'ast syn::ItemUse) {
        self.check_use(&node.tree, None);
        self.check_cli_use(&node.tree, &mut Vec::new());
        visit::visit_item_use(self, node);
    }

    fn visit_ident(&mut self, node: &'ast proc_macro2::Ident) {
        self.check_ident(node);
    }

    fn visit_lit(&mut self, node: &'ast Lit) {
        self.check_lit(node);
    }

    fn visit_macro(&mut self, node: &'ast Macro) {
        if let Some(name) = node.path.segments.last() {
            self.check_macro_name(&name.ident);
        }
        self.check_tokens(node.tokens.clone());
        self.check_expose_tokens(node.tokens.clone());
        self.check_token_paths(node.tokens.clone());
        visit::visit_macro(self, node);
    }
}

fn is_env_read(ident: &syn::Ident) -> bool {
    ENV_READS.iter().any(|read| ident == read)
}

/// The path a `use` leaf imports: `self` stands for its parent module.
fn imported(prefix: &[String], leaf: &syn::Ident) -> Vec<String> {
    let mut full = prefix.to_vec();
    if leaf != "self" {
        full.push(leaf.to_string());
    }
    full
}

/// True when `segments` reach a process spawning API: a `std::process`
/// item other than [`PROCESS_ALLOWED`], or anything under `tokio::process`
/// or `std::os::unix::process`.
fn spawns(segments: &[String]) -> bool {
    segments.windows(2).enumerate().any(|(index, pair)| {
        let [first, second] = pair else { return false };
        if second != "process" {
            return false;
        }
        match first.as_str() {
            "std" => segments
                .get(index + 2)
                .is_some_and(|item| !PROCESS_ALLOWED.contains(&item.as_str())),
            "tokio" | "unix" => true,
            _ => false,
        }
    })
}

/// True when an imported path is the `std::process` module itself or its
/// glob, such as `use std::process as p;` or `use std::process::*;`.
fn imports_process_module(full: &[String]) -> bool {
    matches!(full, [.., std, process] if std == "std" && process == "process")
        || matches!(full, [.., std, process, glob] if std == "std" && process == "process" && glob == "*")
}

/// True when `segments` name the `cli` area module of the error catalog.
fn names_cli_codes(segments: &[String]) -> bool {
    segments
        .windows(2)
        .any(|pair| pair[0] == "codes" && pair[1] == "cli")
}

/// The attributes of the item kinds that can carry `#[cfg(test)]`.
fn item_attrs(item: &Item) -> Option<&[Attribute]> {
    Some(match item {
        Item::Const(item) => &item.attrs,
        Item::Enum(item) => &item.attrs,
        Item::ExternCrate(item) => &item.attrs,
        Item::Fn(item) => &item.attrs,
        Item::ForeignMod(item) => &item.attrs,
        Item::Impl(item) => &item.attrs,
        Item::Macro(item) => &item.attrs,
        Item::Mod(item) => &item.attrs,
        Item::Static(item) => &item.attrs,
        Item::Struct(item) => &item.attrs,
        Item::Trait(item) => &item.attrs,
        Item::TraitAlias(item) => &item.attrs,
        Item::Type(item) => &item.attrs,
        Item::Union(item) => &item.attrs,
        Item::Use(item) => &item.attrs,
        _ => return None,
    })
}

/// True for `#[test]`, `#[cfg(test)]`, and `#[cfg(all(..., test, ...))]`.
fn is_test_only(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attr| {
        if attr.path().is_ident("test") {
            return true;
        }
        if !attr.path().is_ident("cfg") {
            return false;
        }
        attr.parse_args::<Meta>()
            .is_ok_and(|predicate| needs_test(&predicate))
    })
}

fn needs_test(predicate: &Meta) -> bool {
    match predicate {
        Meta::Path(path) => path.is_ident("test"),
        Meta::List(list) if list.path.is_ident("all") => {
            Punctuated::<Meta, Token![,]>::parse_terminated
                .parse2(list.tokens.clone())
                .is_ok_and(|items| items.iter().any(needs_test))
        }
        _ => false,
    }
}

/// The value of `#[path = "..."]`.
fn path_attr(attrs: &[Attribute]) -> Option<String> {
    attrs.iter().find_map(|attr| {
        let Meta::NameValue(name_value) = &attr.meta else {
            return None;
        };
        let Expr::Lit(ExprLit {
            lit: Lit::Str(path),
            ..
        }) = &name_value.value
        else {
            return None;
        };
        name_value.path.is_ident("path").then(|| path.value())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Writes `files` (relative path, source) under a fresh crate folder
    /// and scans it from `src/lib.rs` as `stackly-core`.
    fn scan(files: &[(&str, &str)]) -> Vec<String> {
        scan_as("stackly-core", files)
    }

    /// Like [`scan`], as the crate `name` with its own rule sets.
    fn scan_as(name: &str, files: &[(&str, &str)]) -> Vec<String> {
        let dir = tempfile::tempdir().unwrap();
        for (path, source) in files {
            let path = dir.path().join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, source).unwrap();
        }
        scan_crate(
            dir.path(),
            name,
            &[dir.path().join("src/lib.rs")],
            Rules::for_crate(name),
        )
        .unwrap()
    }

    const ENV_RULE: &str = "library crates never read the environment";
    const PATH_RULE: &str = "library crates never hardcode a machine path";

    #[test]
    fn every_environment_read_is_reported_with_its_file_and_line() {
        let found = scan(&[(
            "src/lib.rs",
            "use std::env;\n\
             pub fn a() { let _ = std::env::var(\"X\"); }\n\
             pub fn b() { let _ = env::var_os(\"X\"); }\n\
             pub fn c() { let _ = std::env::vars().count(); }\n\
             pub fn d() { let _ = env::vars_os(); }\n\
             #[allow(deprecated)] pub fn e() { let _ = env::home_dir(); }\n\
             pub fn f() { let _ = std::env::temp_dir(); }\n",
        )]);

        let lines: Vec<&str> = found
            .iter()
            .map(|message| message.split(": ").next().unwrap())
            .collect();
        assert_eq!(
            lines,
            [2, 3, 4, 5, 6, 7].map(|line| format!("stackly-core src/lib.rs:{line}"))
        );
        assert!(found[0].ends_with(
            "library crates never read the environment (`env::var`); \
             a bin reads it once in main and passes the value in"
        ));
        assert!(found[4].contains("(`env::home_dir`)"));
    }

    #[test]
    fn imported_env_functions_and_reads_inside_macros_are_reported() {
        let found = scan(&[(
            "src/lib.rs",
            "use std::env::{self, var as read};\n\
             use std::env::*;\n\
             pub fn a() -> String { format!(\"{:?}\", std::env::var_os(\"X\")) }\n",
        )]);

        assert_eq!(found.len(), 3, "{found:?}");
        assert!(
            found[0].starts_with("stackly-core src/lib.rs:1: ") && found[0].contains("env::var")
        );
        assert!(found[1].starts_with("stackly-core src/lib.rs:2: ") && found[1].contains("env::*"));
        assert!(
            found[2].starts_with("stackly-core src/lib.rs:3: ") && found[2].contains("env::var_os")
        );
    }

    #[test]
    fn every_machine_path_literal_is_reported_also_inside_macros() {
        let found = scan(&[(
            "src/lib.rs",
            "pub const A: &str = \"/Users/someone\";\n\
             pub const B: &str = \"/opt/homebrew/bin\";\n\
             pub const C: &[u8] = b\"/usr/local\";\n\
             pub fn d(x: &str) -> String { format!(\"/tmp/{x}\") }\n\
             pub const E: &str = r\"/var/folders/xy\";\n",
        )]);

        assert_eq!(found.len(), 5, "{found:?}");
        for (index, path) in MACHINE_PATHS.iter().enumerate() {
            assert_eq!(
                found[index],
                format!(
                    "stackly-core src/lib.rs:{}: {PATH_RULE} (`{path}`); derive it from DirectoryLayout",
                    index + 1
                )
            );
        }
    }

    #[test]
    fn test_code_doc_comments_env_macro_and_namesakes_pass() {
        let found = scan(&[
            (
                "src/lib.rs",
                "//! Roots live under /Users/<name>/Library and /var/folders.\n\
                 mod dirs;\n\
                 #[cfg(all(test, unix))]\n\
                 mod test_support;\n\
                 /// The default is `/opt/homebrew`, see `std::env::var`.\n\
                 pub const VERSION: &str = env!(\"CARGO_PKG_VERSION\");\n\
                 pub fn os() -> &'static str { std::env::consts::OS }\n\
                 pub fn home() -> std::path::PathBuf { dirs::home_dir() }\n\
                 #[cfg(test)]\n\
                 mod tests {\n\
                     #[test]\n\
                     fn reads() { let _ = std::env::var(\"X\"); let _ = \"/tmp\"; }\n\
                 }\n",
            ),
            (
                "src/dirs.rs",
                "use std::path::PathBuf;\n\
                 pub fn home_dir() -> PathBuf { PathBuf::new() }\n\
                 #[test]\n\
                 fn temp() { let _ = std::env::temp_dir(); }\n",
            ),
            (
                "src/test_support.rs",
                "pub const ROOT: &str = \"/private/tmp\";\n",
            ),
        ]);

        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn nested_and_inline_modules_are_followed() {
        let found = scan(&[
            ("src/lib.rs", "mod layout;\nmod inline { mod deep; }\n"),
            ("src/layout/mod.rs", "mod roots;\n"),
            ("src/layout/roots.rs", "pub const A: &str = \"/tmp\";\n"),
            (
                "src/inline/deep.rs",
                "\n\npub fn a() { let _ = std::env::var(\"X\"); }\n",
            ),
        ]);

        assert_eq!(found.len(), 2, "{found:?}");
        assert!(
            found
                .iter()
                .any(|m| m.starts_with("stackly-core src/inline/deep.rs:3: ")
                    && m.contains(ENV_RULE))
        );
        assert!(found.iter().any(
            |m| m.starts_with("stackly-core src/layout/roots.rs:1: ") && m.contains(PATH_RULE)
        ));
    }

    #[test]
    fn a_missing_module_file_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src/lib.rs"), "mod gone;\n").unwrap();

        let err = scan_crate(
            dir.path(),
            "stackly-core",
            &[dir.path().join("src/lib.rs")],
            Rules::for_crate("stackly-core"),
        )
        .unwrap_err();

        assert!(
            err.to_string()
                .starts_with("cannot find the file of `mod gone;`"),
            "{err}"
        );
    }

    #[test]
    fn only_code_that_needs_test_is_skipped() {
        let found = scan(&[(
            "src/lib.rs",
            "#[cfg(not(test))]\n\
             pub fn a() { let _ = std::env::var(\"X\"); }\n\
             #[cfg(any(test, feature = \"x\"))]\n\
             pub const B: &str = \"/usr/local\";\n\
             pub struct S;\n\
             impl S {\n\
                 #[cfg(test)]\n\
                 fn helper() -> &'static str { \"/tmp\" }\n\
                 pub fn c(&self) -> &'static str { \"/opt/homebrew\" }\n\
             }\n\
             #[cfg(test)]\n\
             use std::env::var;\n",
        )]);

        assert_eq!(found.len(), 3, "{found:?}");
        assert!(found[0].starts_with("stackly-core src/lib.rs:2: ") && found[0].contains(ENV_RULE));
        assert!(
            found[1].starts_with("stackly-core src/lib.rs:4: ") && found[1].contains("/usr/local")
        );
        assert!(
            found[2].starts_with("stackly-core src/lib.rs:9: ")
                && found[2].contains("/opt/homebrew")
        );
    }

    #[test]
    fn a_module_loaded_through_a_path_attribute_is_scanned() {
        let found = scan(&[
            (
                "src/lib.rs",
                "#[path = \"generated/paths.rs\"]\nmod paths;\n",
            ),
            (
                "src/generated/paths.rs",
                "pub const A: &str = \"/var/folders/x\";\n",
            ),
        ]);

        assert_eq!(found.len(), 1, "{found:?}");
        assert!(
            found[0].starts_with("stackly-core src/generated/paths.rs:1: "),
            "{found:?}"
        );
    }

    #[test]
    fn a_c_string_literal_and_a_macro_rules_body_are_checked() {
        let found = scan(&[(
            "src/lib.rs",
            "pub const A: &std::ffi::CStr = c\"/tmp/x\";\n\
             macro_rules! root { () => { std::env::temp_dir() }; }\n",
        )]);

        assert_eq!(found.len(), 2, "{found:?}");
        assert!(
            found[0].starts_with("stackly-core src/lib.rs:1: ") && found[0].contains("(`/tmp`)")
        );
        assert!(
            found[1].starts_with("stackly-core src/lib.rs:2: ")
                && found[1].contains("(`env::temp_dir`)")
        );
    }

    const SOCKET: &str = "no tcp or udp sockets (spec 0006)";

    #[test]
    fn every_tcp_or_udp_socket_name_is_reported_once_per_line() {
        let found = scan_as(
            "stackly-ipc",
            &[(
                "src/lib.rs",
                "use std::net::{TcpListener, TcpStream};\n\
                 pub fn a() { let _ = std::net::UdpSocket::bind(\"x\"); }\n\
                 pub type S = tokio::net::TcpSocket;\n\
                 pub fn b() -> String { format!(\"{:?}\", TcpStream::connect(\"x\")) }\n\
                 macro_rules! open { () => { TcpListener::bind(\"x\") }; }\n",
            )],
        );

        assert_eq!(
            found,
            [1, 2, 3, 4, 5].map(|line| format!("stackly-ipc src/lib.rs:{line}: {SOCKET}"))
        );
    }

    #[test]
    fn socket_names_in_strings_doc_comments_and_test_code_pass() {
        let found = scan_as(
            "stacklyd",
            &[(
                "src/lib.rs",
                "//! Never a TcpListener, see AC-5.\n\
                 /// Not a `UdpSocket` either.\n\
                 pub const NOTE: &str = \"TcpStream\";\n\
                 pub struct UnixOnly;\n\
                 #[cfg(test)]\n\
                 mod tests {\n\
                     #[test]\n\
                     fn tcp() { let _ = std::net::TcpListener::bind(\"127.0.0.1:0\"); }\n\
                 }\n",
            )],
        );

        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn each_rule_set_scans_only_its_own_crates() {
        let files = [(
            "src/lib.rs",
            "pub fn a() { let _ = std::env::var(\"X\"); let _ = std::net::TcpStream::connect(\"x\"); }\n",
        )];

        let library = scan_as("stackly-core", &files);
        assert_eq!(library.len(), 1, "{library:?}");
        assert!(library[0].contains("library crates never read the environment"));

        for name in SOCKET_SCANNED {
            assert_eq!(
                scan_as(name, &files),
                [format!("{name} src/lib.rs:1: {SOCKET}")]
            );
        }
        assert!(scan_as("stackly-fake-server", &files).is_empty());
    }

    #[test]
    fn the_socket_rule_scans_the_transport_the_daemon_and_the_cli() {
        assert_eq!(SOCKET_SCANNED, ["stackly-ipc", "stacklyd", "stackly-cli"]);
        assert_eq!(
            Rules::for_crate("stackly-ipc"),
            Rules {
                library: false,
                sockets: true,
                expose_allowed: &[],
                cli: false,
                no_cli_codes: true,
            }
        );
        assert_eq!(
            Rules::for_crate("stackly-storage"),
            Rules {
                library: false,
                sockets: false,
                expose_allowed: &[],
                cli: false,
                no_cli_codes: true,
            }
        );
    }

    const PRINT: &str = "the cli writes through its output module, never print macros, which panic on a broken pipe (spec 0008, AC-10)";
    const CLI_CODES: &str = "only the cli and the protocol name codes::cli, so the daemon never sends a cli code (spec 0008, AC-14)";

    /// The line numbers of the findings with `rule`, in order.
    fn lines_of(found: &[String], rule: &str) -> Vec<usize> {
        found
            .iter()
            .filter(|message| message.ends_with(rule))
            .map(|message| {
                let (_, rest) = message.split_once(".rs:").unwrap();
                rest.split(':').next().unwrap().parse().unwrap()
            })
            .collect()
    }

    #[test]
    fn the_cli_rules_scan_only_the_cli_and_the_code_rule_spares_the_cli_and_the_protocol() {
        let cli = Rules::for_crate("stackly-cli");
        assert!(cli.cli && !cli.no_cli_codes && cli.sockets);
        let protocol = Rules::for_crate("stackly-protocol");
        assert!(!protocol.cli && !protocol.no_cli_codes);
        for name in [
            "stacklyd",
            "stackly-ipc",
            "stackly-testkit",
            "stackly-core",
            "xtask",
        ] {
            let rules = Rules::for_crate(name);
            assert!(!rules.cli && rules.no_cli_codes, "{name}");
        }
    }

    #[test]
    fn every_process_spawning_path_in_the_cli_is_reported_with_its_line() {
        let found = scan_as(
            "stackly-cli",
            &[(
                "src/lib.rs",
                "use std::process::Command;\n\
                 pub fn a() { let _ = std::process::Command::new(\"x\"); }\n\
                 use std::process as p;\n\
                 pub fn b() { let _ = p::Command::new(\"x\"); }\n\
                 use std::process::*;\n\
                 pub fn c() { let _ = tokio::process::Command::new(\"x\"); }\n\
                 use std::os::unix::process::CommandExt;\n\
                 use std::{io, process};\n\
                 use std::process::{self, exit};\n\
                 pub fn d() -> String { format!(\"{:?}\", std::process::Stdio::null()) }\n\
                 use tokio::{process::Child, sync};\n\
                 pub fn e() { let _ = ::std::process::Output::default; }\n",
            )],
        );

        assert_eq!(
            lines_of(&found, SPAWN_RULE),
            [1, 2, 3, 5, 6, 7, 8, 9, 10, 11, 12],
            "{found:?}"
        );
        assert_eq!(found.len(), 11, "{found:?}");
        assert!(found[0].starts_with("stackly-cli src/lib.rs:1: "));
    }

    #[test]
    fn exit_exit_code_id_clap_command_and_test_code_pass_the_spawn_rule() {
        let found = scan_as(
            "stackly-cli",
            &[(
                "src/lib.rs",
                "//! Never `std::process::Command`, and never println!.\n\
                 use std::process::{ExitCode, exit, id as pid};\n\
                 pub fn a() -> std::process::ExitCode { std::process::ExitCode::SUCCESS }\n\
                 pub fn b() -> u32 { std::process::id() }\n\
                 pub fn c() -> ! { std::process::exit(2) }\n\
                 pub fn d() -> clap::Command { clap::Command::new(\"stackly\") }\n\
                 pub const NOTE: &str = \"std::process::Command\";\n\
                 pub fn e() { let _ = format!(\"{}\", std::process::id()); }\n\
                 pub fn f(out: &mut impl std::io::Write) { let _ = writeln!(out, \"x\"); }\n\
                 #[cfg(test)]\n\
                 mod tests {\n\
                     #[test]\n\
                     fn t() { std::process::Command::new(\"x\"); println!(\"x\"); }\n\
                 }\n",
            )],
        );

        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn print_macros_in_the_cli_are_reported_also_inside_other_macros() {
        let found = scan_as(
            "stackly-cli",
            &[(
                "src/lib.rs",
                "pub fn a() { println!(\"x\"); }\n\
                 pub fn b() { eprintln!(\"x\"); }\n\
                 pub fn c() { print!(\"x\"); }\n\
                 pub fn d() { std::eprint!(\"x\"); }\n\
                 pub fn e(v: Option<u8>) { let _ = v.map(|_| { tokio::select! { () = f() => println!(\"x\") } }); }\n\
                 macro_rules! say { () => { println!(\"x\") }; }\n",
            )],
        );

        assert_eq!(lines_of(&found, PRINT), [1, 2, 3, 4, 5, 6], "{found:?}");
        assert_eq!(found.len(), 6, "{found:?}");
        let daemon = scan_as(
            "stacklyd",
            &[("src/lib.rs", "pub fn a() { println!(\"x\"); }\n")],
        );
        assert!(daemon.is_empty(), "{daemon:?}");
    }

    #[test]
    fn codes_cli_outside_the_cli_and_the_protocol_is_reported() {
        let files = [(
            "src/lib.rs",
            "use stackly_protocol::codes::cli;\n\
             use stackly_protocol::codes::{self, ipc, cli as c};\n\
             pub fn a() -> &'static str { stackly_protocol::codes::cli::USAGE.code }\n\
             pub fn b() -> String { format!(\"{}\", codes::cli::PANIC.code) }\n\
             pub fn c() -> &'static str { codes::ipc::CALL_TIMEOUT.code }\n\
             pub const NOTE: &str = \"codes::cli\";\n\
             #[cfg(test)]\n\
             mod tests { fn t() { let _ = codes::cli::USAGE; } }\n",
        )];

        for name in ["stacklyd", "stackly-testkit", "stackly-ipc"] {
            let found = scan_as(name, &files);
            assert_eq!(
                lines_of(&found, CLI_CODES),
                [1, 2, 3, 4],
                "{name}: {found:?}"
            );
            assert_eq!(found.len(), 4, "{name}: {found:?}");
        }
        assert!(scan_as("stackly-protocol", &files).is_empty());
        let cli = scan_as("stackly-cli", &files);
        assert!(cli.is_empty(), "{cli:?}");
    }

    const EXPOSE: &str = "secret expose outside the allowlist";

    #[test]
    fn expose_calls_outside_the_allowlist_are_reported_with_file_and_line() {
        let found = scan(&[
            ("src/lib.rs", "mod secret;\nmod leak;\n"),
            (
                "src/secret/mod.rs",
                "mod redact;\nmod delivery;\nmod generate;\n",
            ),
            (
                "src/secret/redact.rs",
                "pub fn a(v: &V) -> &str { v.expose() }\n",
            ),
            (
                "src/secret/delivery/mod.rs",
                "mod file;\npub fn b(v: &V) -> &str { v.expose() }\n",
            ),
            (
                "src/secret/delivery/file.rs",
                "pub fn c(v: &V) { let _ = format!(\"{}\", v.expose()); }\n",
            ),
            (
                "src/secret/generate.rs",
                "pub fn d(v: &V) -> usize { v.expose().len() }\n",
            ),
            (
                "src/leak.rs",
                "pub fn e(v: &V) { tracing::info!(value = v.expose(), \"x\"); }\n\
                 pub fn f(v: &V) -> String { format!(\"{}\", v.expose()) }\n\
                 pub fn g(v: &V) -> &str { V::expose(v) }\n\
                 pub fn h(vs: &[V]) -> Vec<&str> { vs.iter().map(V::expose).collect() }\n\
                 #[tracing::instrument(fields(secret = v.expose()))]\n\
                 pub fn i(v: &V) {}\n\
                 pub fn j(v: &V) -> &str { v . expose ( ) }\n\
                 pub fn k(v: &V) -> &str { v.expose::<>() }\n\
                 pub fn l(v: &V) { tracing::debug!(\"{}\", Self::expose(v)); }\n",
            ),
        ]);

        let mut lines: Vec<&str> = found
            .iter()
            .map(|message| {
                message
                    .strip_suffix(&format!(": {EXPOSE}"))
                    .unwrap_or(message)
            })
            .collect();
        lines.sort_unstable();
        assert_eq!(
            lines,
            [
                "stackly-core src/leak.rs:1",
                "stackly-core src/leak.rs:2",
                "stackly-core src/leak.rs:3",
                "stackly-core src/leak.rs:4",
                "stackly-core src/leak.rs:5",
                "stackly-core src/leak.rs:7",
                "stackly-core src/leak.rs:8",
                "stackly-core src/leak.rs:9",
                "stackly-core src/secret/generate.rs:1",
            ]
        );
    }

    #[test]
    fn expose_in_test_code_doc_comments_strings_and_a_definition_passes() {
        let found = scan_as(
            "stackly-storage",
            &[(
                "src/lib.rs",
                "//! Read it with `value.expose()`.\n\
                 /// Never call v.expose() here.\n\
                 pub const NOTE: &str = \"v.expose()\";\n\
                 pub struct V;\n\
                 impl V { pub fn expose(&self) -> &str { \"\" } }\n\
                 pub struct W { pub exposed: bool }\n\
                 #[test]\n\
                 fn t() { V.expose(); }\n\
                 #[cfg(all(test, unix))]\n\
                 mod tests { fn u() { tracing::info!(\"{}\", super::V.expose()); } }\n",
            )],
        );

        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn an_allowed_module_covers_its_inline_and_nested_modules_only() {
        let found = scan(&[(
            "src/lib.rs",
            "mod secret {\n\
                 mod delivery { mod argv { fn a(v: &V) { v.expose(); } } }\n\
                 mod service { fn b(v: &V) { v.expose(); } }\n\
             }\n\
             mod redact { fn c(v: &V) { v.expose(); } }\n",
        )]);

        assert_eq!(
            found,
            [
                format!("stackly-core src/lib.rs:3: {EXPOSE}"),
                format!("stackly-core src/lib.rs:5: {EXPOSE}"),
            ]
        );
    }

    #[test]
    fn the_testkit_fake_secret_module_may_expose_and_no_other_crate_or_module_may() {
        let files = [
            ("src/lib.rs", "mod fake_secret;\nmod other;\n"),
            (
                "src/fake_secret.rs",
                "pub fn a(v: &V) -> String { v.expose().to_owned() }\n",
            ),
            ("src/other.rs", "pub fn b(v: &V) -> &str { v.expose() }\n"),
        ];

        assert_eq!(
            scan_as("stackly-testkit", &files),
            [format!("stackly-testkit src/other.rs:1: {EXPOSE}")]
        );
        let mut storage = scan_as("stackly-storage", &files);
        storage.sort();
        assert_eq!(
            storage,
            [
                format!("stackly-storage src/fake_secret.rs:1: {EXPOSE}"),
                format!("stackly-storage src/other.rs:1: {EXPOSE}"),
            ]
        );
    }

    #[test]
    fn the_expose_allowlist_is_the_call_sites_that_exist() {
        assert_eq!(
            EXPOSE_ALLOWED,
            [
                ("stackly-core", &["secret::redact", "secret::delivery"][..]),
                ("stackly-platform-macos", &["secret", "process::stdin"][..]),
                ("stackly-protocol", &["secret"][..]),
                ("stackly-testkit", &["fake_secret"][..]),
            ]
        );
    }

    #[test]
    fn the_scanned_crates_are_the_five_the_spec_names() {
        assert_eq!(
            SCANNED,
            [
                "stackly-domain",
                "stackly-platform",
                "stackly-core",
                "stackly-protocol",
                "stackly-platform-macos",
            ]
        );
    }
}
