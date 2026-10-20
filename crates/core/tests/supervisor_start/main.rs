//! The executor of the process supervisor on fakes (spec 0010, steps 7 and
//! 8): init, the startup transaction, stop, readiness and health, and
//! reload, over `FakeProcessSupervision`, `FakeRegistry`, and the real dummy
//! server for the probes.

mod common;
mod init;
mod reload;
mod start;
mod status;
mod stop;
