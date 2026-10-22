//! Creating an instance (spec 0020): the draft, the facts [`gather`] reads,
//! the pure [`evaluate`] that preflight and the create precheck share, the
//! form model, and the `create_instance` operation.

mod check;
mod command;
mod compensator;
mod draft;
mod error;
mod evaluate;
mod facts;
mod form;
mod ids;
mod port;
mod work;

pub use check::{Check, CheckKind, CheckStatus, CreateError, FieldCode, FieldIssue};
pub use command::CommandSpec;
pub use compensator::{
    INSTANCE_REGISTRATION, InstanceRegistrationCompensator, InstanceRemoved, NoInstanceRemoved,
    registration_key,
};
pub use draft::{
    CREATE_MIN_FREE_BYTES, CreateDraft, NAME_SUGGESTION_MAX_SUFFIX, PORT_SUGGESTION_ATTEMPTS,
    PORT_SUGGESTION_MIN, PREFLIGHT_PLACEHOLDER_ID, PortRequestDraft, ProviderChoice,
    SecretSourceKind, SecretValueShape, placeholder_instance_id,
};
pub use error::PinFailure;
pub use evaluate::{Evaluation, ResolvedCreate, SecretPlan, evaluate, evaluate_for};
pub use facts::{
    CreateDeps, CreateFacts, DataFacts, DirFact, GatherError, PackageSource, ProbeFact,
    ProgramCheck, ProviderFound, ProviderProblem, SecretFacts, SourceFact, gather,
};
pub use form::{
    CreateFormModel, DATA_PATH_TEXT, FormEndpoint, FormError, FormProvider, build_form,
    build_form_from_package, names_of, select_provider, suggested_name,
};
#[cfg(feature = "test-support")]
pub use ids::ScriptedIds;
pub use ids::{IdSource, SystemIds, fresh_instance_id};
pub use port::PortFacts;
pub use work::{
    CreateHooks, CreateInstanceWork, CreateWorkDeps, CreateWorkFactory, Hook, SourceRecorder,
    encode_input,
};
