//! The source scan of `cargo xtask check-deps` (spec 0004, "Peta modul dan
//! dependency"): library crates never read environment variables and never
//! hardcode a machine path. Bins read `STACKLY_ROOT` once in `main`, and
//! every other path comes from `DirectoryLayout`.
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

/// The crates whose source is scanned.
pub const SCANNED: [&str; 5] = [
    "stackly-domain",
    "stackly-platform",
    "stackly-core",
    "stackly-protocol",
    "stackly-platform-macos",
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

/// Scans the lib and bin targets of `package`. Messages read
/// `<crate> <file>:<line>: <rule>`, with `file` relative to `root`.
///
/// # Errors
///
/// Returns an error when a source file cannot be read or parsed, or a
/// `mod` declaration names a file that does not exist.
pub fn scan_package(root: &Path, package: &Package) -> Result<Vec<String>> {
    let entries: Vec<PathBuf> = package
        .targets
        .iter()
        .filter(|target| target.is_lib() || target.is_bin())
        .map(|target| target.src_path.clone().into_std_path_buf())
        .collect();
    scan_crate(root, package.name.as_str(), &entries)
}

/// Scans one crate from its root source files.
fn scan_crate(root: &Path, name: &str, entries: &[PathBuf]) -> Result<Vec<String>> {
    let mut pending: Vec<(PathBuf, PathBuf)> = entries
        .iter()
        .map(|entry| (entry.clone(), parent(entry)))
        .collect();
    let mut seen: Vec<PathBuf> = Vec::new();
    let mut violations = Vec::new();
    while let Some((file, module_dir)) = pending.pop() {
        if seen.contains(&file) {
            continue;
        }
        seen.push(file.clone());
        let source =
            fs::read_to_string(&file).with_context(|| format!("cannot read {}", file.display()))?;
        let syntax = syn::parse_file(&source)
            .with_context(|| format!("cannot parse {} as Rust", file.display()))?;
        let mut scan = Scan {
            file_dir: parent(&file),
            module_dir,
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
}

impl OutOfLine {
    /// Returns the module's file and the folder its own child modules sit
    /// in.
    fn resolve(self) -> Result<(PathBuf, PathBuf)> {
        if let Some(path) = self.path {
            // A file loaded through `#[path]` keeps its children beside it.
            let children = parent(&path);
            return Ok((path, children));
        }
        let flat = self.dir.join(format!("{}.rs", self.name));
        if flat.is_file() {
            let children = self.dir.join(&self.name);
            return Ok((flat, children));
        }
        let nested = self.dir.join(&self.name).join("mod.rs");
        if nested.is_file() {
            let children = self.dir.join(&self.name);
            return Ok((nested, children));
        }
        anyhow::bail!(
            "cannot find the file of `mod {};` in {}",
            self.name,
            self.dir.display()
        )
    }
}

struct Scan {
    /// The folder of the file being scanned.
    file_dir: PathBuf,
    /// The folder the file's own child modules sit in.
    module_dir: PathBuf,
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
        self.report(
            span,
            format!(
                "library crates never read the environment (`env::{name}`); a bin reads it once in main and passes the value in"
            ),
        );
    }

    fn check_text(&mut self, span: proc_macro2::Span, text: &str) {
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

    /// Reads a macro body token by token: string literals, and
    /// `env :: <name>` sequences.
    fn check_tokens(&mut self, tokens: TokenStream) {
        let tokens: Vec<TokenTree> = tokens.into_iter().collect();
        for (index, token) in tokens.iter().enumerate() {
            match token {
                TokenTree::Group(group) => self.check_tokens(group.stream()),
                TokenTree::Literal(literal) => self.check_lit(&Lit::new(literal.clone())),
                TokenTree::Ident(ident) if ident == "env" => {
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
        self.modules.push(OutOfLine {
            dir: self.child_dir(),
            name,
            path,
        });
    }

    fn visit_attribute(&mut self, _node: &'ast Attribute) {
        // Attributes are metadata, and doc comments may name any path.
    }

    fn visit_path(&mut self, node: &'ast syn::Path) {
        let segments: Vec<&syn::Ident> = node.segments.iter().map(|s| &s.ident).collect();
        if let [.., env, name] = segments.as_slice()
            && *env == "env"
            && is_env_read(name)
        {
            self.env_read(name.span(), &name.to_string());
        }
        visit::visit_path(self, node);
    }

    fn visit_item_use(&mut self, node: &'ast syn::ItemUse) {
        self.check_use(&node.tree, None);
    }

    fn visit_lit(&mut self, node: &'ast Lit) {
        self.check_lit(node);
    }

    fn visit_macro(&mut self, node: &'ast Macro) {
        self.check_tokens(node.tokens.clone());
        visit::visit_macro(self, node);
    }
}

fn is_env_read(ident: &syn::Ident) -> bool {
    ENV_READS.iter().any(|read| ident == read)
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
        let dir = tempfile::tempdir().unwrap();
        for (path, source) in files {
            let path = dir.path().join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, source).unwrap();
        }
        scan_crate(dir.path(), "stackly-core", &[dir.path().join("src/lib.rs")]).unwrap()
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

        let err =
            scan_crate(dir.path(), "stackly-core", &[dir.path().join("src/lib.rs")]).unwrap_err();

        assert!(
            err.to_string()
                .starts_with("cannot find the file of `mod gone;`"),
            "{err}"
        );
    }
}
