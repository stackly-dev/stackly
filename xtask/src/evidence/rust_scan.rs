//! The Rust half of `evidence check`: the native test markers (spec 0002,
//! "Penanda test native").
//!
//! Each file is parsed with `syn`, so attributes split over several lines are
//! read whole. A file fails when:
//!
//! - `#[ignore]` has no reason starting with `native: `;
//! - a `cfg_attr` holds `ignore`;
//! - an `#[ignore]` function has no `evidence!` with `Kind::Native`;
//! - an `evidence!` with `Kind::Native` sits in a function without
//!   `#[ignore]`.
//!
//! `evidence!` is matched by the last segment of its path, so
//! `crate::evidence!` and `stackly_testkit::evidence::evidence!` count too.
//! Tests written by another macro (`proptest!`, `rstest`) are not visible
//! here, which is why native tests must be plain `#[test]` functions.

use syn::parse::Parser;
use syn::punctuated::Punctuated;
use syn::visit::{self, Visit};
use syn::{Attribute, Expr, ExprLit, Ident, Lit, Macro, Meta, Token};

/// The reason prefix every `#[ignore]` carries.
const NATIVE_PREFIX: &str = "native: ";

/// Returns every rule violation in one Rust source file. `file` is the
/// repository relative path used in the messages.
pub fn scan(file: &str, source: &str) -> Vec<String> {
    let syntax = match syn::parse_file(source) {
        Ok(syntax) => syntax,
        Err(err) => return vec![format!("{file}: cannot parse as Rust: {err}")],
    };
    let mut scan = Scan {
        file,
        functions: Vec::new(),
        violations: Vec::new(),
    };
    scan.visit_file(&syntax);
    scan.violations
}

/// The function a node sits in.
struct Function {
    name: String,
    ignored: bool,
    native_evidence: bool,
}

struct Scan<'a> {
    file: &'a str,
    /// The functions around the current node, innermost last. Closures are
    /// part of the function that holds them.
    functions: Vec<Function>,
    violations: Vec<String>,
}

impl Scan<'_> {
    fn report(&mut self, message: impl std::fmt::Display) {
        let location = match self.functions.last() {
            Some(function) => format!("{}: fn `{}`", self.file, function.name),
            None => self.file.to_owned(),
        };
        self.violations.push(format!("{location}: {message}"));
    }

    /// Checks the `#[ignore]` of one function, visits its body, then checks
    /// that an ignored function wrote native evidence.
    fn function(&mut self, attrs: &[Attribute], ident: &Ident, body: impl FnOnce(&mut Self)) {
        let ignore = attrs.iter().find(|attr| attr.path().is_ident("ignore"));
        self.functions.push(Function {
            name: ident.to_string(),
            ignored: ignore.is_some(),
            native_evidence: false,
        });
        if let Some(attr) = ignore
            && !has_native_reason(attr)
        {
            self.report(format!(
                "#[ignore] is only for native tests; write #[ignore = \"{NATIVE_PREFIX}<what it needs>\"]"
            ));
        }
        body(self);
        let function = self
            .functions
            .last()
            .expect("the function pushed above is still on the stack");
        if function.ignored && !function.native_evidence {
            self.report("an #[ignore] test needs evidence!(..., kind = Kind::Native, ...)");
        }
        self.functions.pop();
    }
}

impl<'ast> Visit<'ast> for Scan<'_> {
    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        self.function(&node.attrs, &node.sig.ident, |scan| {
            visit::visit_item_fn(scan, node);
        });
    }

    fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
        self.function(&node.attrs, &node.sig.ident, |scan| {
            visit::visit_impl_item_fn(scan, node);
        });
    }

    fn visit_trait_item_fn(&mut self, node: &'ast syn::TraitItemFn) {
        self.function(&node.attrs, &node.sig.ident, |scan| {
            visit::visit_trait_item_fn(scan, node);
        });
    }

    fn visit_attribute(&mut self, node: &'ast Attribute) {
        if node.path().is_ident("cfg_attr") && cfg_attr_holds_ignore(&node.meta) {
            self.report("#[cfg_attr(..., ignore)] is not allowed; native tests use #[ignore = \"native: ...\"]");
        }
        visit::visit_attribute(self, node);
    }

    fn visit_macro(&mut self, node: &'ast Macro) {
        if is_evidence_macro(node) && has_native_kind(node) {
            match self.functions.last_mut() {
                Some(function) if function.ignored => function.native_evidence = true,
                _ => self.report(
                    "evidence! with Kind::Native belongs only in an #[ignore = \"native: ...\"] test",
                ),
            }
        }
        visit::visit_macro(self, node);
    }
}

/// True for `#[ignore = "native: <something>"]`.
fn has_native_reason(attr: &Attribute) -> bool {
    let Meta::NameValue(name_value) = &attr.meta else {
        return false;
    };
    let Expr::Lit(ExprLit {
        lit: Lit::Str(reason),
        ..
    }) = &name_value.value
    else {
        return false;
    };
    reason
        .value()
        .strip_prefix(NATIVE_PREFIX)
        .is_some_and(|what| !what.trim().is_empty())
}

/// True when a `cfg_attr` adds `ignore`, also through a nested `cfg_attr`.
fn cfg_attr_holds_ignore(meta: &Meta) -> bool {
    let Meta::List(list) = meta else {
        return false;
    };
    let parser = Punctuated::<Meta, Token![,]>::parse_terminated;
    match parser.parse2(list.tokens.clone()) {
        // The first item is the predicate; the rest are the attributes.
        Ok(items) => items.iter().skip(1).any(|item| {
            item.path().is_ident("ignore")
                || (item.path().is_ident("cfg_attr") && cfg_attr_holds_ignore(item))
        }),
        // Not plain attribute syntax: fall back to looking for the word.
        Err(_) => list
            .tokens
            .to_string()
            .split(|c: char| !(c.is_alphanumeric() || c == '_'))
            .any(|word| word == "ignore"),
    }
}

fn is_evidence_macro(mac: &Macro) -> bool {
    mac.path
        .segments
        .last()
        .is_some_and(|segment| segment.ident == "evidence")
}

/// True when the macro has a `kind = ...Kind::Native` argument.
fn has_native_kind(mac: &Macro) -> bool {
    let Ok(arguments) = mac.parse_body_with(Punctuated::<Expr, Token![,]>::parse_terminated) else {
        return false;
    };
    arguments.iter().any(|argument| {
        let Expr::Assign(assign) = argument else {
            return false;
        };
        let (Expr::Path(name), Expr::Path(value)) = (&*assign.left, &*assign.right) else {
            return false;
        };
        let segments: Vec<String> = value
            .path
            .segments
            .iter()
            .map(|segment| segment.ident.to_string())
            .collect();
        name.path.is_ident("kind") && segments.ends_with(&["Kind".to_owned(), "Native".to_owned()])
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn messages(source: &str) -> Vec<String> {
        scan("tests/native_example.rs", source)
    }

    #[test]
    fn a_native_test_with_native_evidence_passes() {
        let source = r#"
            #[test]
            #[ignore = "native: needs a managed PostgreSQL 18 runtime"]
            fn postgres_reaches_ready() {
                let ev = evidence!(
                    covers = ["AC-009"],
                    kind = Kind::Native,
                    expected = "PostgreSQL 18 reaches ready",
                );
                ev.pass_with("ready");
            }

            mod tests {
                #[test]
                fn fixture_test() {
                    let ev = crate::evidence!(covers = ["REQ-051"], kind = Kind::Fixture, expected = "x");
                    ev.pass();
                }
            }
        "#;
        assert_eq!(messages(source), Vec::<String>::new());
    }

    #[test]
    fn ignore_without_a_native_reason_fails() {
        let source = r#"
            #[test]
            #[ignore]
            fn bare() { let ev = evidence!(covers = ["AC-001"], kind = Kind::Native, expected = "x"); }

            #[test]
            #[ignore = "slow"]
            fn slow() { let ev = evidence!(covers = ["AC-001"], kind = Kind::Native, expected = "x"); }

            #[test]
            #[ignore = "native: "]
            fn empty_reason() { let ev = evidence!(covers = ["AC-001"], kind = Kind::Native, expected = "x"); }
        "#;
        let found = messages(source);
        assert_eq!(found.len(), 3, "{found:?}");
        assert!(found[0].starts_with("tests/native_example.rs: fn `bare`: #[ignore] is only"));
        assert!(found[1].contains("fn `slow`"));
        assert!(found[2].contains("fn `empty_reason`"));
    }

    #[test]
    fn cfg_attr_with_ignore_fails_also_when_nested() {
        let source = r#"
            #[test]
            #[cfg_attr(not(target_os = "macos"), ignore)]
            fn only_on_mac() {}

            #[test]
            #[cfg_attr(unix, cfg_attr(not(target_os = "macos"), ignore = "native: x"))]
            fn nested() {}

            #[cfg_attr(docsrs, doc(cfg(feature = "full")))]
            fn unrelated() {}
        "#;
        let found = messages(source);
        assert_eq!(found.len(), 2, "{found:?}");
        assert!(found[0].contains("fn `only_on_mac`: #[cfg_attr(..., ignore)]"));
        assert!(found[1].contains("fn `nested`"));
    }

    #[test]
    fn an_ignored_test_without_native_evidence_fails() {
        let source = r#"
            #[test]
            #[ignore = "native: needs NATS"]
            fn no_evidence() {}

            #[test]
            #[ignore = "native: needs NATS"]
            fn fixture_evidence() {
                let ev = evidence!(covers = ["AC-001"], kind = Kind::Fixture, expected = "x");
            }

            #[test]
            #[ignore = "native: needs NATS"]
            fn in_a_nested_function() {
                fn helper() {
                    let ev = evidence!(covers = ["AC-001"], kind = Kind::Native, expected = "x");
                }
            }
        "#;
        let found = messages(source);
        assert_eq!(found.len(), 4, "{found:?}");
        assert!(found[0].contains("fn `no_evidence`: an #[ignore] test needs evidence!"));
        assert!(found[1].contains("fn `fixture_evidence`: an #[ignore] test needs"));
        assert!(found[2].contains("fn `helper`: evidence! with Kind::Native belongs only"));
        assert!(found[3].contains("fn `in_a_nested_function`: an #[ignore] test needs"));
    }

    #[test]
    fn native_evidence_outside_an_ignored_test_fails() {
        let source = r#"
            #[test]
            fn not_ignored() {
                let ev = stackly_testkit::evidence::evidence!(
                    covers = ["AC-001"],
                    kind = stackly_testkit::evidence::Kind::Native,
                    expected = "x",
                );
            }

            #[test]
            fn closure_counts_as_the_test() {
                let check = || evidence!(covers = ["AC-001"], kind = Kind::Native, expected = "x");
            }

            #[test]
            fn other_uses_of_native_are_fine() {
                assert_eq!(Kind::parse("native"), Some(Kind::Native));
            }
        "#;
        let found = messages(source);
        assert_eq!(found.len(), 2, "{found:?}");
        assert!(found[0].contains("fn `not_ignored`: evidence! with Kind::Native"));
        assert!(found[1].contains("fn `closure_counts_as_the_test`"));
    }

    #[test]
    fn a_file_that_is_not_rust_is_reported() {
        let found = messages("fn broken( {");
        assert_eq!(found.len(), 1);
        assert!(found[0].starts_with("tests/native_example.rs: cannot parse as Rust"));
    }
}
