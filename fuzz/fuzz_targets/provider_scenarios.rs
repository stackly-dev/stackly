//! Fuzz target `provider_scenarios` (spec 0036, AC-3): random bytes as
//! `tests/scenarios.yaml`, parsed and validated against the fake package's
//! settings schema; every rejection names that file and every accepted
//! file holds 1 to 16 uniquely named scenarios.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    stackly_core::provider::fuzzing::scenarios(data);
});
