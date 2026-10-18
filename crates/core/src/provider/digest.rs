//! `manifest_sha256`: the digest of the files that decide a provider's
//! behavior.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use sha2::{Digest, Sha256};
use stackly_domain::Sha256Digest;

/// Hashes the listed files of a package: SHA-256 over one line
/// `<file sha256, lowercase hex>  <relative path>\n` per file, sorted by the
/// path's bytes.
///
/// The caller passes `provider.yaml`, `schemas/settings.schema.json`, and
/// every template a config file references, once each, with the same bytes
/// it parsed.
pub(crate) fn manifest_sha256<'a>(
    files: impl IntoIterator<Item = (&'a str, &'a [u8])>,
) -> Sha256Digest {
    let sorted: BTreeMap<&str, &[u8]> = files.into_iter().collect();
    let mut listing = String::new();
    for (path, bytes) in sorted {
        let digest = Sha256::digest(bytes);
        for byte in digest {
            // Writing to a String cannot fail.
            let _ = write!(listing, "{byte:02x}");
        }
        listing.push_str("  ");
        listing.push_str(path);
        listing.push('\n');
    }
    Sha256Digest::from_bytes(Sha256::digest(listing.as_bytes()).into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_digest_hashes_a_sorted_listing_of_file_digests() {
        let digest = manifest_sha256([("b", b"2".as_slice()), ("a", b"1".as_slice())]);

        // sha256("1") and sha256("2"), listed in path order.
        let listing = "6b86b273ff34fce19d6b804eff5a3f5747ada4eaa22f1d49c01e52ddb7875b4b  a\n\
                       d4735e3a265e16eee03f59718b9b5d03019c07d8b6c51f90da3a666eec13ab35  b\n";
        assert_eq!(
            digest,
            Sha256Digest::from_bytes(Sha256::digest(listing.as_bytes()).into())
        );
    }

    #[test]
    fn changing_one_byte_changes_the_digest_and_order_does_not() {
        let base = manifest_sha256([("a", b"x".as_slice()), ("b", b"y".as_slice())]);
        let swapped = manifest_sha256([("b", b"y".as_slice()), ("a", b"x".as_slice())]);
        let changed = manifest_sha256([("a", b"x".as_slice()), ("b", b"z".as_slice())]);

        assert_eq!(base, swapped);
        assert_ne!(base, changed);
    }
}
