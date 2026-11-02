//! The executor of the process supervisor on fakes (spec 0010, steps 7 to
//! 11): init, the startup transaction, stop, readiness and health, reload,
//! the restart policy of a crash outside any operation, and
//! reconciliation after a daemon restart, over `FakeProcessSupervision`, `FakeRegistry`, and the real dummy
//! server for the probes.

mod actor_commit;
mod common;
mod config;
mod data_compat;
mod force_restart;
mod init;
mod integrity;
mod launch;
mod listener;
mod native;
mod pid_reuse;
mod reconcile;
mod reload;
mod restart;
mod spool;
mod start;
mod status;
mod stop;
mod wake;
