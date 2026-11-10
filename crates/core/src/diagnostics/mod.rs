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

/// Reading a bundle back, for the tests of the crates that call this module;
/// only `stackly-core` reads zip files (spec 0018).
#[cfg(feature = "test-support")]
pub mod testing {
    use std::collections::BTreeMap;
    use std::io::{Cursor, Read};

    /// Every entry of the zip `bytes`, decompressed, by path.
    ///
    /// # Panics
    ///
    /// Panics when `bytes` is not a zip whose entries all read.
    #[must_use]
    pub fn unzip(bytes: &[u8]) -> BTreeMap<String, Vec<u8>> {
        let mut archive =
            zip::ZipArchive::new(Cursor::new(bytes.to_vec())).expect("the bundle is a zip");
        let mut entries = BTreeMap::new();
        for index in 0..archive.len() {
            let mut file = archive.by_index(index).expect("a zip entry reads");
            let name = file
                .name()
                .expect("a zip entry has a UTF-8 name")
                .into_owned();
            let mut content = Vec::new();
            file.read_to_end(&mut content)
                .expect("a zip entry decompresses");
            entries.insert(name, content);
        }
        entries
    }
}
