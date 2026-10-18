//! The `--name=value` flag parser every command shares.

use std::collections::{BTreeMap, BTreeSet};
use std::net::{IpAddr, Ipv4Addr};
use std::path::PathBuf;
use std::str::FromStr;

use crate::Failure;

/// The flags one command received, already checked against the names it
/// accepts.
#[derive(Debug)]
pub(crate) struct Flags {
    values: BTreeMap<String, String>,
    switches: BTreeSet<String>,
}

impl Flags {
    /// Parses `args`, where every entry is `--name=value` for a name in
    /// `valued` or a bare `--name` for a name in `switches`. An unknown
    /// name, a missing or extra value, a flag given twice, or a positional
    /// argument is a usage failure.
    pub(crate) fn parse(
        args: &[String],
        valued: &[&str],
        switches: &[&str],
    ) -> Result<Self, Failure> {
        let mut values = BTreeMap::new();
        let mut set = BTreeSet::new();
        for arg in args {
            let Some(flag) = arg.strip_prefix("--") else {
                return Err(usage(format!("unexpected argument {arg:?}")));
            };
            let fresh = match flag.split_once('=') {
                Some((name, value)) => {
                    if switches.contains(&name) {
                        return Err(usage(format!("--{name} takes no value")));
                    }
                    if !valued.contains(&name) {
                        return Err(usage(format!("unknown flag --{name}")));
                    }
                    values.insert(name.to_owned(), value.to_owned()).is_none()
                }
                None => {
                    if valued.contains(&flag) {
                        return Err(usage(format!(
                            "--{flag} needs a value, written --{flag}=<value>"
                        )));
                    }
                    if !switches.contains(&flag) {
                        return Err(usage(format!("unknown flag --{flag}")));
                    }
                    set.insert(flag.to_owned())
                }
            };
            if !fresh {
                let name = flag.split_once('=').map_or(flag, |(name, _)| name);
                return Err(usage(format!("--{name} is given more than once")));
            }
        }
        Ok(Self {
            values,
            switches: set,
        })
    }

    /// True when the bare switch `--name` was given.
    pub(crate) fn switch(&self, name: &str) -> bool {
        self.switches.contains(name)
    }

    /// The path in `--name`, which must be given and not be empty.
    pub(crate) fn required_path(&self, name: &str) -> Result<PathBuf, Failure> {
        match self.values.get(name) {
            Some(value) if !value.is_empty() => Ok(PathBuf::from(value)),
            Some(_) => Err(usage(format!("--{name} must not be empty"))),
            None => Err(usage(format!("--{name}=<path> is required"))),
        }
    }

    /// The non negative whole number in `--name`, or `default` when the
    /// flag is absent.
    pub(crate) fn number<T: FromStr>(&self, name: &str, default: T) -> Result<T, Failure> {
        self.values.get(name).map_or(Ok(default), |value| {
            value
                .parse()
                .map_err(|_| usage(format!("--{name} must be a whole number, got {value:?}")))
        })
    }

    /// The `true` or `false` in `--name`, or `default` when the flag is
    /// absent.
    pub(crate) fn boolean(&self, name: &str, default: bool) -> Result<bool, Failure> {
        match self.values.get(name).map(String::as_str) {
            None => Ok(default),
            Some("true") => Ok(true),
            Some("false") => Ok(false),
            Some(other) => Err(usage(format!(
                "--{name} must be true or false, got {other:?}"
            ))),
        }
    }

    /// The TCP port in `--port`, which must be given and be 1 to 65535.
    pub(crate) fn port(&self) -> Result<u16, Failure> {
        let Some(value) = self.values.get("port") else {
            return Err(usage("--port=<port> is required".to_owned()));
        };
        match value.parse::<u16>() {
            Ok(port) if port != 0 => Ok(port),
            _ => Err(usage(format!("--port must be 1 to 65535, got {value:?}"))),
        }
    }

    /// The loopback address in `--host`, `127.0.0.1` when absent. The fake
    /// server never listens beyond loopback, like every Stackly endpoint.
    pub(crate) fn host(&self) -> Result<IpAddr, Failure> {
        let Some(value) = self.values.get("host") else {
            return Ok(IpAddr::V4(Ipv4Addr::LOCALHOST));
        };
        match value.parse::<IpAddr>() {
            Ok(host) if host.is_loopback() => Ok(host),
            Ok(_) => Err(usage(format!(
                "--host must be a loopback address, got {value:?}"
            ))),
            Err(_) => Err(usage(format!(
                "--host must be an ip address, got {value:?}"
            ))),
        }
    }
}

fn usage(message: String) -> Failure {
    Failure::Usage(message)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|arg| (*arg).to_owned()).collect()
    }

    fn parse(list: &[&str]) -> Result<Flags, Failure> {
        Flags::parse(
            &args(list),
            &["data", "fail", "port", "host"],
            &["token-stdin"],
        )
    }

    fn usage_message(list: &[&str]) -> String {
        match parse(list) {
            Err(Failure::Usage(message)) => message,
            other => panic!("expected a usage failure for {list:?}, got {other:?}"),
        }
    }

    #[test]
    fn a_value_keeps_everything_after_the_first_equals_sign() {
        let flags = parse(&["--data=/tmp/a=b c", "--token-stdin"]).unwrap();

        assert_eq!(
            flags.required_path("data").unwrap(),
            PathBuf::from("/tmp/a=b c")
        );
        assert!(flags.switch("token-stdin"));
    }

    #[test]
    fn absent_optional_flags_take_their_defaults() {
        let flags = parse(&[]).unwrap();

        assert!(!flags.switch("token-stdin"));
        assert!(!flags.boolean("fail", false).unwrap());
        assert_eq!(flags.number("port", 7_u64).unwrap(), 7);
        assert_eq!(flags.host().unwrap(), IpAddr::V4(Ipv4Addr::LOCALHOST));
    }

    #[test]
    fn unknown_repeated_positional_and_misshapen_flags_are_usage_failures() {
        assert_eq!(usage_message(&["--nope=1"]), "unknown flag --nope");
        assert_eq!(usage_message(&["--nope"]), "unknown flag --nope");
        assert_eq!(usage_message(&["data"]), "unexpected argument \"data\"");
        assert_eq!(
            usage_message(&["--token-stdin=yes"]),
            "--token-stdin takes no value"
        );
        assert_eq!(
            usage_message(&["--data"]),
            "--data needs a value, written --data=<value>"
        );
        assert_eq!(
            usage_message(&["--data=a", "--data=b"]),
            "--data is given more than once"
        );
        assert_eq!(
            usage_message(&["--token-stdin", "--token-stdin"]),
            "--token-stdin is given more than once"
        );
    }

    #[test]
    fn booleans_accept_only_true_and_false() {
        assert!(
            parse(&["--fail=true"])
                .unwrap()
                .boolean("fail", false)
                .unwrap()
        );
        assert!(
            !parse(&["--fail=false"])
                .unwrap()
                .boolean("fail", true)
                .unwrap()
        );
        assert!(matches!(
            parse(&["--fail=yes"]).unwrap().boolean("fail", false),
            Err(Failure::Usage(_))
        ));
    }

    #[test]
    fn port_must_be_given_and_be_one_to_65535() {
        assert_eq!(parse(&["--port=1"]).unwrap().port().unwrap(), 1);
        assert_eq!(parse(&["--port=65535"]).unwrap().port().unwrap(), 65535);
        for bad in ["--port=0", "--port=65536", "--port=abc", "--port=-1"] {
            assert!(
                matches!(parse(&[bad]).unwrap().port(), Err(Failure::Usage(_))),
                "{bad}"
            );
        }
        assert!(matches!(parse(&[]).unwrap().port(), Err(Failure::Usage(_))));
    }

    #[test]
    fn host_must_be_a_loopback_address() {
        assert_eq!(
            parse(&["--host=::1"]).unwrap().host().unwrap(),
            "::1".parse::<IpAddr>().unwrap()
        );
        for bad in ["--host=0.0.0.0", "--host=192.168.1.2", "--host=localhost"] {
            assert!(
                matches!(parse(&[bad]).unwrap().host(), Err(Failure::Usage(_))),
                "{bad}"
            );
        }
    }

    #[test]
    fn a_required_path_must_be_given_and_not_be_empty() {
        assert!(matches!(
            parse(&[]).unwrap().required_path("data"),
            Err(Failure::Usage(_))
        ));
        assert!(matches!(
            parse(&["--data="]).unwrap().required_path("data"),
            Err(Failure::Usage(_))
        ));
    }

    #[test]
    fn numbers_reject_text_and_negative_values() {
        let flags = parse(&["--port=abc"]).unwrap();

        assert!(matches!(
            flags.number("port", 0_u64),
            Err(Failure::Usage(_))
        ));
        let flags = parse(&["--port=-5"]).unwrap();
        assert!(matches!(
            flags.number("port", 0_u64),
            Err(Failure::Usage(_))
        ));
    }
}
