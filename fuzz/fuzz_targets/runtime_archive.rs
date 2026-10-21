//! Fuzz target `runtime_archive` (spec 0018, AC-19): the first byte picks
//! `tar_gz`, `zip`, or `binary` and whether a folder is stripped, the rest
//! is an artifact the extractor reads into a tree in memory.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    stackly_core::runtime::fuzzing::archive(data);
});
