//! The planned daemon restart through the real daemon server (spec 0030
//! AC-6, AC-7, AC-9 to AC-14): the plan the preview shows, the quiesce, the
//! stop of the instances the plan stops, the abort, and the daemon that
//! comes after. The daemon runs in process with the fake provider and a
//! handover list the test chooses, and a restart is one daemon that exits
//! and the next one on the same root ([`harness::RestartHarness`]).
#![cfg(target_os = "macos")]

#[path = "common/mod.rs"]
mod common;
#[path = "restart_plan/errors.rs"]
mod errors;
#[path = "restart_plan/harness.rs"]
mod harness;
#[path = "restart_plan/plan.rs"]
mod plan;
#[path = "restart_plan/restore.rs"]
mod restore;
#[path = "restart_plan/summary.rs"]
mod summary;
#[path = "supervisor/support.rs"]
mod support;
