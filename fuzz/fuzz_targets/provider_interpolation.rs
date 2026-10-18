//! Fuzz target `provider_interpolation` (spec 0009): random text as a
//! manifest field, checked in every position; an accepted string must keep
//! the rule of its position.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    stackly_core::provider::fuzzing::interpolation(data);
});
