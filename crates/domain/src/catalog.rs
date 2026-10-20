//! The provider catalog: how well a provider version is supported on a
//! target (PRD section 5.1, spec 0017).

use crate::code::code_enum;

code_enum! {
    /// The support status of a provider version on a target (PRD section
    /// 5.1). `supported` needs a passing compatibility record (BR-025); the
    /// catalog rules that pick a status live in `stackly-core` (spec 0017),
    /// and feature 18 reuses this type for installed runtimes.
    #[non_exhaustive]
    pub enum SupportStatus: SupportStatus {
        /// On the roadmap, not built yet.
        Planned => "planned",
        /// Available, but not through every check it needs.
        Unverified => "unverified",
        /// May be tried with a warning; not part of a stable release
        /// promise.
        Experimental => "experimental",
        /// The version, OS, architecture, and capability combination
        /// passed the test matrix.
        Supported => "supported",
        /// Known not to be available or not supported by Stackly.
        Unsupported => "unsupported",
        /// A prerequisite is not met, with a recorded reason.
        Blocked => "blocked",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn support_status_codes_are_the_prd_status_names_in_snake_case() {
        let codes: Vec<&str> = SupportStatus::ALL.iter().map(|s| s.as_code()).collect();

        assert_eq!(
            codes,
            [
                "planned",
                "unverified",
                "experimental",
                "supported",
                "unsupported",
                "blocked"
            ]
        );
        for status in SupportStatus::ALL {
            assert_eq!(status.as_code().parse::<SupportStatus>().unwrap(), *status);
        }
        assert!("tested".parse::<SupportStatus>().is_err());
    }
}
