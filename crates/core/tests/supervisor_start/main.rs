//! The executor of the process supervisor on fakes (spec 0010, steps 7 to
//! 9): init, the startup transaction, stop, readiness and health, reload,
//! and the restart policy of a crash outside any operation, over `FakeProcessSupervision`, `FakeRegistry`, and the real dummy
//! server for the probes.

mod common;
mod init;
mod reload;
mod restart;
mod spool;
mod start;
mod status;
mod stop;
