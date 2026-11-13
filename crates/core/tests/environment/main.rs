//! Environment manifests (spec 0044): loading and validation against a
//! catalog of the six official packages and the fake one, then the plan.

mod common;
mod down;
mod down_edges;
mod edges;
mod export;
mod limits;
mod local_edges;
mod lock;
mod lock_edges;
mod plan;
mod plan_local;
mod plan_lock;
mod register;
mod site;
mod status;
mod up;
mod up_edges;
mod up_fake;
mod validate;
