//! The environment, read once at start (spec 0008, *Urutan saat start*):
//! `STACKLY_ROOT` through `MacosPlatform::new` with the daemon's own rules,
//! then `STACKLY_NO_INPUT`, then `TERM` for the progress mode. Nothing is
//! created: the CLI only works out where the daemon's socket is.
//!
//! `NO_COLOR`, `CLICOLOR`, `CLICOLOR_FORCE`, and `TZ` are read by
//! `anstream` and jiff, not here.

use std::ffi::OsStr;
use std::path::PathBuf;

use stackly_platform::ROOT_OVERRIDE_ENV;

use crate::failure::CliFailure;

/// The variable that turns noninteractive mode on.
pub const NO_INPUT_ENV: &str = "STACKLY_NO_INPUT";

/// Where the daemon is, and what the environment asks of the CLI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Environment {
    /// The daemon socket of the active roots.
    pub socket: PathBuf,
    /// The uid that must own the socket and the daemon peer.
    pub uid: u32,
    /// `STACKLY_NO_INPUT` is on.
    pub no_input: bool,
    /// `TERM` is `dumb`.
    pub term_is_dumb: bool,
}

/// Reads the environment of this process.
///
/// # Errors
///
/// `cli.invalid_environment` when the root resolver refuses `STACKLY_ROOT`
/// or `STACKLY_NO_INPUT` holds an unknown value, and
/// `cli.platform_unavailable` when the roots or the socket path cannot be
/// worked out for another reason.
pub fn read() -> Result<Environment, CliFailure> {
    let (socket, uid) = platform::endpoint(std::env::var_os(ROOT_OVERRIDE_ENV))?;
    let no_input = parse_no_input(std::env::var_os(NO_INPUT_ENV).as_deref())?;
    let term_is_dumb = std::env::var_os("TERM").is_some_and(|term| term == "dumb");
    Ok(Environment {
        socket,
        uid,
        no_input,
        term_is_dumb,
    })
}

/// `STACKLY_NO_INPUT`: `1` or `true` in any case turns it on; unset,
/// empty, `0`, or `false` leaves it off.
///
/// # Errors
///
/// `cli.invalid_environment` for any other value, so a typo never means
/// interactive.
pub fn parse_no_input(value: Option<&OsStr>) -> Result<bool, CliFailure> {
    let Some(value) = value else {
        return Ok(false);
    };
    match value.to_str().map(str::to_ascii_lowercase).as_deref() {
        Some("" | "0" | "false") => Ok(false),
        Some("1" | "true") => Ok(true),
        _ => Err(CliFailure::invalid_environment(
            NO_INPUT_ENV,
            "use 1 or true to turn it on, 0 or false to turn it off",
        )),
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use std::ffi::OsString;
    use std::path::PathBuf;

    use stackly_platform::{DirectoryLayout, PlatformError, ROOT_OVERRIDE_ENV, UserId};
    use stackly_platform_macos::MacosPlatform;

    use crate::failure::CliFailure;

    /// The socket path and the uid that must own it, from the same roots
    /// the daemon resolves.
    pub(super) fn endpoint(root_override: Option<OsString>) -> Result<(PathBuf, u32), CliFailure> {
        let platform = MacosPlatform::new(root_override).map_err(|error| failure(&error))?;
        let roots = platform
            .directories()
            .roots()
            .map_err(|error| failure(&error))?;
        let uid = match platform
            .identity()
            .current_user()
            .map_err(|error| failure(&error))?
        {
            UserId::Unix(uid) => uid,
            _ => {
                return Err(CliFailure::platform_unavailable(
                    "no_unix_uid",
                    "the current user has no unix uid",
                ));
            }
        };
        Ok((DirectoryLayout::new(roots).daemon_socket(), uid))
    }

    fn failure(error: &PlatformError) -> CliFailure {
        let reason = match error {
            PlatformError::InvalidRootOverride { reason, .. } => {
                return CliFailure::invalid_environment(ROOT_OVERRIDE_ENV, reason);
            }
            PlatformError::HomeDirUnavailable => "home_unavailable",
            PlatformError::TempDirUnavailable => "temp_unavailable",
            PlatformError::RuntimeRootTooLong { .. } => "socket_path_too_long",
            _ => "roots_unavailable",
        };
        CliFailure::platform_unavailable(reason, &error.to_string())
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    use std::ffi::OsString;
    use std::path::PathBuf;

    use crate::failure::CliFailure;

    /// Only macOS has a daemon for now.
    pub(super) fn endpoint(_root_override: Option<OsString>) -> Result<(PathBuf, u32), CliFailure> {
        Err(CliFailure::platform_unavailable(
            "unsupported_os",
            "the daemon only runs on macos for now",
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use stackly_protocol::codes::cli;

    use super::*;

    #[test]
    fn no_input_takes_1_or_true_in_any_case_and_refuses_unknown_values() {
        for (value, expected) in [
            (None, Some(false)),
            (Some(""), Some(false)),
            (Some("0"), Some(false)),
            (Some("false"), Some(false)),
            (Some("FALSE"), Some(false)),
            (Some("1"), Some(true)),
            (Some("true"), Some(true)),
            (Some("TRUE"), Some(true)),
            (Some("yes"), None),
            (Some("2"), None),
            (Some(" 1"), None),
        ] {
            let parsed = parse_no_input(value.map(OsStr::new));
            match expected {
                Some(on) => assert_eq!(parsed, Ok(on), "{value:?}"),
                None => {
                    let failure = parsed.unwrap_err();
                    assert!(failure.body.is(&cli::INVALID_ENVIRONMENT), "{value:?}");
                    assert_eq!(failure.body.detail("variable"), Some(NO_INPUT_ENV));
                    assert_eq!(failure.exit_code(), 2);
                }
            }
        }
    }

    #[test]
    fn a_no_input_value_that_is_not_utf8_is_refused() {
        use std::os::unix::ffi::OsStringExt;
        let value = OsString::from_vec(vec![0xff, b'1']);
        assert!(parse_no_input(Some(&value)).is_err());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_relative_root_override_is_an_invalid_environment() {
        let failure = platform::endpoint(Some(OsString::from("relative/root"))).unwrap_err();
        assert!(failure.body.is(&cli::INVALID_ENVIRONMENT));
        assert_eq!(failure.body.detail("variable"), Some(ROOT_OVERRIDE_ENV));
        assert!(
            failure.body.message().contains("not absolute"),
            "{failure:?}"
        );
    }
}
