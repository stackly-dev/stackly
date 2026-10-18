//! Fuzz target `provider_settings_schema` (spec 0009): random bytes as a
//! settings schema, then fixed documents; accepted values must validate
//! again unchanged.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    stackly_core::provider::fuzzing::settings_schema(data);
});
