//! The executor of the process supervisor on fakes (spec 0010, steps 7 to
//! 11): init, the startup transaction, stop, readiness and health, reload,
//! the restart policy of a crash outside any operation, and
//! reconciliation after a daemon restart, over `FakeProcessSupervision`, `FakeRegistry`, and the real dummy
//! server for the probes.

mod actor_commit;
mod audit_notes;
mod common;
mod config;
mod data_compat;
mod doctor;
mod force_restart;
mod init;
mod integrity;
mod launch;
mod leases;
mod listener;
mod login;
mod native;
mod pid_reuse;
mod reconcile;
mod reload;
mod restart;
mod secret_retry;
mod session_end;
mod spool;
mod start;
mod status;
mod stop;
mod stop_all;
mod visible;
mod wake;
