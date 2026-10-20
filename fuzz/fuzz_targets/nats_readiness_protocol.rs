//! Fuzz target `nats_readiness_protocol` (spec 0023): random bytes from a
//! server through the `nats-readiness` exchange, for both JetStream
//! expectations and chunk sizes from 1 to 128 bytes.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    stackly_core::provider::fuzzing::nats_readiness_protocol(data);
});
