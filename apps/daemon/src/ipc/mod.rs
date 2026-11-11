//! The handlers of the use case methods, one module per area, each with
//! the DTO mapping its area needs (spec 0007, `ipc::operation`).

pub mod agent_off;
pub mod audit;
pub mod diagnostics;
pub mod doctor;
pub mod instance;
pub mod logs;
pub mod operation;
pub mod provider;
pub mod restart;
pub mod runtime;
pub mod secret;
pub mod settings;
pub mod stop_all;
