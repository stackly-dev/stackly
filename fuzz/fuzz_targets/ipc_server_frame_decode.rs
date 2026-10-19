//! Fuzz target `ipc_server_frame_decode` (spec 0006): random bytes into the
//! staged daemon frame decoder every Rust client uses; a decoded frame must
//! survive a round trip.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    stackly_protocol::fuzzing::server_frame_decode(data);
});
