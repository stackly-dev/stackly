//! Fuzz target `runtime_macho` (spec 0018, AC-19): the bytes as a thin or
//! universal Mach-O file read by the load command parser.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    stackly_core::runtime::fuzzing::macho(data);
});
