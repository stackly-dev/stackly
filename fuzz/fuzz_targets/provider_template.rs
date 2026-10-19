//! Fuzz target `provider_template` (spec 0009): the first byte picks a
//! package and format, the rest is a template; valid output must parse in
//! its format.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    stackly_core::provider::fuzzing::template(data);
});
