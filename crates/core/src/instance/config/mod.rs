//! Changing an instance that already exists (spec 0029): the pure config
//! evaluator that `instance.config.get`, `instance.config.validate`, and
//! the apply precheck share, so all three always give the same answer for
//! the same facts, the facts it reads, and the history copies and drift
//! check of the rendered config files.

pub mod codes;
mod error;
mod evaluate;
mod gather;
mod plan;
mod preview;

pub use error::ConfigError;
pub use evaluate::{
    ChangeEvaluation, ConfigImpact, SettingChange, diff_settings, evaluate_change, impact_of,
};
pub use gather::{ConfigFacts, RuntimeFact, gather_config};
pub use plan::{
    ChangePlan, ConfigChange, EndpointIssue, Planned, Rejected, plan_change, refusal_error,
};
pub use preview::{EffectiveFile, changed_lines, preview};

/// How many config revisions of one instance the registry keeps (spec
/// 0029, AC-12); the active one and the last known good always stay.
pub const CONFIG_HISTORY_KEEP: u32 = 20;
