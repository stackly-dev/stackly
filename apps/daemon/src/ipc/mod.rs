//! The handlers of the use case methods, one module per area, each with
//! the DTO mapping its area needs (spec 0007, `ipc::operation`).

pub mod instance;
pub mod operation;
pub mod runtime;
pub mod secret;
