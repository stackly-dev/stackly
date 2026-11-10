//! The diagnostic bundle (spec 0034, *Isi bundle*, *Staging preview*,
//! AC-12, AC-13): a zip the daemon builds for a preview, keeps staged in
//! its cache, and copies byte for byte to a path the client chose.
//!
//! The daemon gathers the entries (the doctor report, its DTOs, the logs)
//! and this module redacts, masks the home folder, holds them to their
//! limits, writes the zip, stages it, and exports it. Nothing here reads a
//! secret value: the `Redactor` only knows the values the daemon already
//! learned. Nothing is ever sent over the network.

mod bundle;
mod export;
mod home;
mod logs;
mod staging;

pub use bundle::{
    AgentFile, BUNDLE_BYTES_MAX, BuiltBundle, BundleEntryKind, BundleEntrySummary, BundleError,
    BundleInput, DAEMON_LOG_BYTES_MAX, DAEMON_LOG_LINES_MAX, INSTANCE_LOG_BYTES_MAX,
    INSTANCE_LOG_ENTRIES_MAX, INSTANCE_LOGS_MAX, InstanceLogInput, LogText, build,
    suggested_file_name,
};
pub use export::{
    DESTINATION_MAX, DestinationProblem, ExportError, Exported, FILE_NAME_MAX, check_destination,
    export,
};
pub use home::HomeMask;
pub use logs::{daemon_log, instance_log};
pub use staging::{PREVIEW_TTL, PREVIEWS_MAX, StagedPreview, Staging, StagingError, WriteReason};
