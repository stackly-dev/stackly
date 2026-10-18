//! Fuzz target `ipc_request_decode` (spec 0006): random bytes into the
//! staged request decoder; an accepted request must survive a round trip.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    stackly_protocol::fuzzing::request_decode(data);
});
