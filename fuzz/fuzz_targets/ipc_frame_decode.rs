//! Fuzz target `ipc_frame_decode` (spec 0006): random bytes, cut at random
//! boundaries, into the frame decoder with the 1 MiB control frame limit.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    stackly_protocol::fuzzing::frame_decode(data);
});
