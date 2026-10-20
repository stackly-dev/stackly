//! The archive format of a runtime artifact in `versions.json`.

use crate::code::code_enum;

code_enum! {
    /// How a runtime artifact is packed (spec 0009 `versions.json`, field
    /// `format`).
    pub enum ArtifactFormat: ArtifactFormat {
        /// A gzip compressed tar archive.
        TarGz => "tar_gz",
        /// A zip archive.
        Zip => "zip",
        /// A single executable, not an archive.
        Binary => "binary",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_format_round_trips_through_its_code() {
        for format in ArtifactFormat::ALL {
            assert_eq!(format.as_code().parse::<ArtifactFormat>().unwrap(), *format);
        }
        assert_eq!(ArtifactFormat::TarGz.to_string(), "tar_gz");
    }

    #[test]
    fn unknown_formats_are_rejected() {
        for code in ["tar", "tar.gz", "TarGz", "dmg", ""] {
            assert!(
                code.parse::<ArtifactFormat>().is_err(),
                "{code} was accepted"
            );
        }
    }
}
