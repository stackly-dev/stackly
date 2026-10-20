//! The codes the supervisor fails an operation with and the `supervisor`
//! and `process` areas of the protocol catalog list the same names (spec
//! 0010 build step 12). A code on one side only would reach `stacklyd`
//! without an entry to map it to, or sit in the catalog with nothing to
//! raise it.

use std::collections::BTreeSet;

use stackly_core::supervisor::codes;
use stackly_protocol::codes::{process, supervisor};

fn core_codes() -> BTreeSet<&'static str> {
    codes::ALL.iter().map(|code| code.as_str()).collect()
}

fn catalog_codes() -> BTreeSet<&'static str> {
    supervisor::ALL
        .iter()
        .chain(process::ALL)
        .map(|spec| spec.code)
        .collect()
}

#[test]
fn every_supervisor_and_process_code_of_core_is_in_the_protocol_catalog() {
    let catalog = catalog_codes();
    for code in core_codes() {
        assert!(catalog.contains(code), "`{code}` has no catalog entry");
    }
}

#[test]
fn every_supervisor_and_process_code_of_the_catalog_is_raised_by_core() {
    let core = core_codes();
    for code in catalog_codes() {
        assert!(core.contains(code), "nothing in core raises `{code}`");
    }
}

#[test]
fn the_two_lists_have_the_same_number_of_codes() {
    assert_eq!(core_codes().len(), codes::ALL.len(), "core repeats a code");
    assert_eq!(core_codes().len(), catalog_codes().len());
}
