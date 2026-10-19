//! Fuzz target `provider_manifest` (spec 0009): random bytes as
//! `provider.yaml` next to the fake package's other files; a valid package
//! must keep every plan invariant.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    stackly_core::provider::fuzzing::manifest(data);
});
