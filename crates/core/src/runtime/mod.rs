//! Managed runtimes (spec 0018): the safe extractor for runtime artifacts,
//! and the sample builders its tests and fuzz seeds share.
//!
//! The extractor in [`archive`] reads a verified artifact as a stream and
//! writes it through a [`stackly_platform::TreeWriter`], with the
//! normalization, symlink, and size rules of spec 0018 *Ekstraksi aman*.
//! It never calls the unpack functions of the archive crates and never
//! writes an extended attribute.
//!
//! [`download`] brings an artifact into the cache through the
//! [`stackly_platform::Fetcher`] port, resuming a partial file and keeping
//! only bytes whose size and digest match the catalog.

pub mod archive;
pub mod closure;
pub mod codes;
pub mod download;
pub mod launch;
pub mod macho;
#[doc(hidden)]
pub mod samples;
pub mod validate;
