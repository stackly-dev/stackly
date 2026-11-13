//! `environment::export` (spec 0045): AC-14, AC-15.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use stackly_core::environment::export::{
    ExportEndpoint, ExportFormat, ExportService, ExportSkipReason, ExportSource, collect, render,
};
use stackly_core::secret::Redactor;
use stackly_domain::Port;
use stackly_platform::SecretValue;

fn ip(name: &str, host: IpAddr, port: Option<u16>, auto: bool) -> ExportEndpoint {
    ExportEndpoint::Ip {
        name: name.parse().expect("an endpoint name"),
        host,
        port: port.map(|p| Port::new(p).expect("a port")),
        auto,
    }
}

fn service(alias: &str, source: ExportSource) -> ExportService {
    ExportService {
        alias: alias.parse().expect("an alias"),
        source,
    }
}

const LOOPBACK: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);

fn services() -> Vec<ExportService> {
    vec![
        service(
            "postgres",
            ExportSource::Instance {
                running: true,
                endpoints: vec![
                    ip("client", LOOPBACK, Some(5432), false),
                    ExportEndpoint::Unix {
                        name: "socket".parse().expect("a name"),
                    },
                ],
            },
        ),
        service(
            "cache",
            ExportSource::Instance {
                running: false,
                endpoints: vec![
                    ip("client", LOOPBACK, Some(49152), true),
                    ip("bus", IpAddr::V6(Ipv6Addr::LOCALHOST), None, true),
                ],
            },
        ),
        service("queue", ExportSource::NotCreated),
        service("old", ExportSource::Retained),
        service("lost", ExportSource::PackageUnavailable),
        service(
            "web-api",
            ExportSource::Instance {
                running: true,
                endpoints: vec![ip(
                    "http",
                    IpAddr::V6(Ipv6Addr::LOCALHOST),
                    Some(8080),
                    false,
                )],
            },
        ),
        // `web` + `api-http` names the same variables as `web-api` + `http`.
        service(
            "web",
            ExportSource::Instance {
                running: true,
                endpoints: vec![ip("api-http", LOOPBACK, Some(8081), false)],
            },
        ),
    ]
}

// covers: AC-14
#[test]
fn every_endpoint_with_a_port_gives_three_variables_in_both_formats() {
    let collected = collect(&services(), &Redactor::new()).expect("safe values");
    let dotenv = render(&collected.variables, ExportFormat::Dotenv);
    assert_eq!(
        dotenv,
        "STACKLY_POSTGRES_CLIENT_HOST=127.0.0.1\n\
         STACKLY_POSTGRES_CLIENT_PORT=5432\n\
         STACKLY_POSTGRES_CLIENT_ADDRESS=127.0.0.1:5432\n\
         STACKLY_CACHE_CLIENT_HOST=127.0.0.1\n\
         STACKLY_CACHE_CLIENT_PORT=49152\n\
         STACKLY_CACHE_CLIENT_ADDRESS=127.0.0.1:49152\n\
         STACKLY_WEB_API_HTTP_HOST=::1\n\
         STACKLY_WEB_API_HTTP_PORT=8080\n\
         STACKLY_WEB_API_HTTP_ADDRESS=[::1]:8080\n"
    );
    let shell = render(&collected.variables, ExportFormat::Shell);
    assert!(shell.starts_with("export STACKLY_POSTGRES_CLIENT_HOST='127.0.0.1'\n"));
    assert_eq!(shell.lines().count(), 9);

    let may_change: Vec<_> = collected
        .variables
        .iter()
        .filter(|v| v.may_change)
        .map(|v| v.name.as_str())
        .collect();
    assert_eq!(
        may_change,
        [
            "STACKLY_CACHE_CLIENT_HOST",
            "STACKLY_CACHE_CLIENT_PORT",
            "STACKLY_CACHE_CLIENT_ADDRESS"
        ]
    );
    let skipped: Vec<_> = collected
        .skipped
        .iter()
        .map(|s| {
            (
                s.service.to_string(),
                s.endpoint.as_ref().map(ToString::to_string),
                s.reason,
            )
        })
        .collect();
    let some = |text: &str| Some(text.to_owned());
    assert_eq!(
        skipped,
        [
            (
                "postgres".to_owned(),
                some("socket"),
                ExportSkipReason::UnixEndpoint
            ),
            (
                "cache".to_owned(),
                some("bus"),
                ExportSkipReason::PortNotAssigned
            ),
            ("queue".to_owned(), None, ExportSkipReason::NotCreated),
            ("old".to_owned(), None, ExportSkipReason::Retained),
            (
                "lost".to_owned(),
                None,
                ExportSkipReason::PackageUnavailable
            ),
            (
                "web".to_owned(),
                some("api-http"),
                ExportSkipReason::NameCollision
            ),
            (
                "web".to_owned(),
                some("api-http"),
                ExportSkipReason::NameCollision
            ),
            (
                "web".to_owned(),
                some("api-http"),
                ExportSkipReason::NameCollision
            ),
        ]
    );
}

// covers: AC-15
#[test]
fn a_value_the_redactor_knows_is_skipped_and_never_in_the_text() {
    let redactor = Redactor::new();
    redactor.learn(&SecretValue::new("127.0.0.1:5432".to_owned()).expect("a value"));
    let collected = collect(&services(), &redactor).expect("safe values");
    let text = render(&collected.variables, ExportFormat::Dotenv);
    assert!(!text.contains("127.0.0.1:5432"), "{text}");
    assert!(
        collected
            .skipped
            .iter()
            .any(|s| s.reason == ExportSkipReason::Redacted
                && s.endpoint.as_ref().map(ToString::to_string).as_deref() == Some("client")
                && s.service.as_str() == "postgres")
    );
    assert!(
        collected
            .variables
            .iter()
            .all(|v| v.name != "STACKLY_POSTGRES_CLIENT_ADDRESS")
    );
}
