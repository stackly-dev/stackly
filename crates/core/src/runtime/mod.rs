//! Managed runtimes (spec 0018): the safe extractor for runtime artifacts,
//! and the sample builders its tests and fuzz seeds share.
//!
//! The extractor in [`archive`] reads a verified artifact as a stream and
//! writes it through a [`stackly_platform::TreeWriter`], with the
//! normalization, symlink, and size rules of spec 0018 *Ekstraksi aman*.
//! It never calls the unpack functions of the archive crates and never
//! writes an extended attribute.

pub mod archive;
#[doc(hidden)]
pub mod samples;
