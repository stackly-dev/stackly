//! `package_sha256`: the digest of every file of a provider package, the
//! bytes an approval binds (spec 0037, *Digest paket*).

use std::fmt::Write as _;

use sha2::{Digest, Sha256};
use stackly_domain::Sha256Digest;

use crate::provider::PackageFiles;

/// Hashes every file of `files`: SHA-256 over one line
/// `<file sha256, lowercase hex>  <relative path>\n` per file, sorted by the
/// path's bytes.
///
/// It has the form of `manifest_sha256` (spec 0009) but covers every file
/// the loader read, `versions.json` included. Because `provider.yaml`
/// holds `metadata.id` and `metadata.revision`, it also binds the ID, and
/// with it the publisher, and the revision.
#[must_use]
pub fn package_sha256(files: &PackageFiles) -> Sha256Digest {
    // `PackageFiles` iterates a sorted map, so the paths come in byte order.
    let mut listing = String::new();
    for (path, bytes) in files.iter() {
        for byte in Sha256::digest(bytes) {
            // Writing to a String cannot fail.
            let _ = write!(listing, "{byte:02x}");
        }
        listing.push_str("  ");
        listing.push_str(path);
        listing.push('\n');
    }
    Sha256Digest::from_bytes(Sha256::digest(listing.as_bytes()).into())
}

/// Returns the size of the package: the bytes of every file together.
#[must_use]
pub fn package_bytes(files: &PackageFiles) -> u64 {
    files.iter().map(|(_, bytes)| bytes.len() as u64).sum()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    fn files(entries: &[(&str, &[u8])]) -> PackageFiles {
        PackageFiles::new(
            "demo",
            entries
                .iter()
                .map(|(path, bytes)| ((*path).to_owned(), bytes.to_vec()))
                .collect::<BTreeMap<_, _>>(),
        )
    }

    #[test]
    fn the_digest_hashes_a_sorted_listing_of_every_file() {
        let digest = package_sha256(&files(&[("b", b"2"), ("a", b"1")]));

        // sha256("1") and sha256("2"), listed in path order.
        let listing = "6b86b273ff34fce19d6b804eff5a3f5747ada4eaa22f1d49c01e52ddb7875b4b  a\n\
                       d4735e3a265e16eee03f59718b9b5d03019c07d8b6c51f90da3a666eec13ab35  b\n";
        assert_eq!(
            digest,
            Sha256Digest::from_bytes(Sha256::digest(listing.as_bytes()).into())
        );
    }

    #[test]
    fn a_change_in_any_file_changes_the_digest_and_the_folder_name_does_not() {
        let base = package_sha256(&files(&[("provider.yaml", b"x"), ("versions.json", b"y")]));
        let versions = package_sha256(&files(&[("provider.yaml", b"x"), ("versions.json", b"z")]));
        let renamed = package_sha256(&PackageFiles::new(
            "elsewhere",
            [("provider.yaml", b"x"), ("versions.json", b"y")]
                .into_iter()
                .map(|(path, bytes)| (path.to_owned(), bytes.to_vec()))
                .collect(),
        ));

        assert_ne!(base, versions, "versions.json is covered");
        assert_eq!(base, renamed);
        assert_eq!(package_bytes(&files(&[("a", b"12"), ("b", b"345")])), 5);
    }
}
