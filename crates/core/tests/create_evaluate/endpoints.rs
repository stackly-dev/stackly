//! The endpoint rows and the port suggestion (spec 0020 AC-4, AC-5,
//! *Pemeriksaan port*).

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use stackly_core::instance::create::CheckStatus;
use stackly_core::supervisor::BindOutcome;
use stackly_domain::{Endpoint, InstanceLifecycle, PortRequest};
use stackly_evidence::{Kind, evidence};

use crate::common::{Site, auto, error_codes, fixed, has_row, paths, row};

const V4: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);
const V6: IpAddr = IpAddr::V6(Ipv6Addr::LOCALHOST);
const V4_ANY: IpAddr = IpAddr::V4(Ipv4Addr::UNSPECIFIED);
const V6_ANY: IpAddr = IpAddr::V6(Ipv6Addr::UNSPECIFIED);

fn addr(host: IpAddr, port: u16) -> SocketAddr {
    SocketAddr::new(host, port)
}

#[test]
fn an_endpoint_without_a_request_is_auto_and_is_not_probed() {
    let site = Site::new();

    let evaluation = site.evaluate(&site.draft("orders"));

    let http = row(&evaluation, "endpoint.http");
    assert_eq!(http.status, CheckStatus::Ok);
    assert_eq!(http.detail, "Port assigned when the instance starts");
    assert_eq!(http.title, "Endpoint 127.0.0.1 · http");
    assert!(
        site.probe.asked().is_empty(),
        "an auto port is never probed"
    );
    let resolved = evaluation.resolved.expect("ready");
    let Endpoint::Ip(ip) = &resolved.endpoints[0] else {
        panic!("an ip endpoint");
    };
    assert_eq!(ip.request(), PortRequest::Auto);
    assert_eq!(ip.address(), V4);
}

#[test]
fn a_free_fixed_port_runs_steps_four_to_six_on_the_exact_address_wildcards_and_other_loopback() {
    let ev = evidence!(
        covers = ["VAL-006"],
        kind = Kind::Fixture,
        expected = "a free fixed port is bound with SO_REUSEADDR on 127.0.0.1, 0.0.0.0, [::1], and then [::], and shows ok",
    );
    let site = Site::new();
    let mut draft = site.draft("orders");
    draft.endpoints = vec![fixed("http", 20000)];

    let evaluation = site.evaluate(&draft);

    let http = row(&evaluation, "endpoint.http");
    assert_eq!(http.status, CheckStatus::Ok, "{http:?}");
    assert_eq!(http.title, "Endpoint 127.0.0.1:20000");
    assert_eq!(http.detail, "Free on IPv4 and IPv6 loopback");
    assert_eq!(
        site.probe.asked(),
        [
            (addr(V4, 20000), true),
            (addr(V4_ANY, 20000), true),
            (addr(V6, 20000), true),
            (addr(V6_ANY, 20000), true),
        ]
    );
    ev.pass();
}

#[test]
fn a_port_the_exact_address_holds_is_in_use_with_a_suggestion_that_skips_taken_ports() {
    let ev = evidence!(
        covers = ["BR-005"],
        kind = Kind::Fixture,
        expected = "a fixed port in use on the exact address is supervisor.endpoint_in_use with the first free port after it as suggestedPort, never applied to the draft",
    );
    let site = Site::new();
    site.probe.answer(addr(V4, 20000), true, BindOutcome::InUse);
    site.probe.answer(addr(V4, 20001), true, BindOutcome::InUse);
    // A wildcard holder makes 20002 a caution, which a suggestion skips.
    site.probe
        .answer(addr(V4_ANY, 20002), true, BindOutcome::InUse);
    let mut draft = site.draft("orders");
    draft.endpoints = vec![fixed("http", 20000)];

    let evaluation = site.evaluate(&draft);

    assert_eq!(error_codes(&evaluation), ["supervisor.endpoint_in_use"]);
    let http = row(&evaluation, "endpoint.http");
    assert_eq!(http.suggested_port, Some(20003));
    assert_eq!(http.detail, "In use by another process");
    assert_eq!(paths(&evaluation), [("endpoints[0].port", "invalid_value")]);
    assert!(evaluation.resolved.is_none());
    assert_eq!(
        draft.endpoints,
        [fixed("http", 20000)],
        "the draft keeps the port the user typed"
    );
    ev.pass();
}

#[test]
fn a_bind_the_os_refuses_is_endpoint_unavailable_with_its_reason() {
    let site = Site::new();
    let mut draft = site.draft("orders");
    draft.endpoints = vec![fixed("http", 80)];
    site.probe.answer(addr(V4, 80), true, BindOutcome::Denied);

    let evaluation = site.evaluate(&draft);
    assert_eq!(
        error_codes(&evaluation),
        ["supervisor.endpoint_unavailable"]
    );
    let stackly_core::instance::create::CreateError::EndpointUnavailable { reason, port, .. } =
        evaluation.first_error().expect("an error").clone()
    else {
        panic!("endpoint unavailable");
    };
    assert_eq!((reason, port.get()), ("denied", 80));
    assert_eq!(
        row(&evaluation, "endpoint.http").detail,
        "macOS won’t let Stackly use this port"
    );
    // The suggestion starts at 1024 for a port below it.
    assert_eq!(row(&evaluation, "endpoint.http").suggested_port, Some(1024));

    site.probe
        .answer(addr(V4, 80), true, BindOutcome::Unavailable);
    let evaluation = site.evaluate(&draft);
    let stackly_core::instance::create::CreateError::EndpointUnavailable { reason, .. } =
        evaluation.first_error().expect("an error").clone()
    else {
        panic!("endpoint unavailable");
    };
    assert_eq!(reason, "unavailable");
}

const ALL_ADDRESSES: &str =
    "Also in use on all addresses of this Mac; clients may reach that server";
const UNCHECKED: &str = "Couldn’t check this port";

/// The status, detail, and error of the `http` row of a draft that asks
/// `port` on `site`, with `answers` set on its probe first.
fn judge_with(
    site: &Site,
    port: u16,
    answers: &[(IpAddr, BindOutcome)],
) -> (CheckStatus, String, Vec<&'static str>) {
    for (host, outcome) in answers {
        site.probe.answer(addr(*host, port), true, *outcome);
    }
    let mut draft = site.draft("orders");
    draft.endpoints = vec![fixed("http", port)];
    let evaluation = site.evaluate(&draft);
    let http = row(&evaluation, "endpoint.http");
    (http.status, http.detail.clone(), error_codes(&evaluation))
}

#[test]
fn another_address_of_the_port_is_a_caution_never_an_error_and_never_ok() {
    let ev = evidence!(
        covers = ["VAL-006"],
        kind = Kind::Fixture,
        expected = "a wildcard or other loopback holder of a fixed port, found by binds with SO_REUSEADDR, is a caution with a note, a probe the OS refuses on those is a caution that the port could not be checked, and none is an error",
    );
    let draft_port = 20000;

    let (status, detail, errors) =
        judge_with(&Site::new(), draft_port, &[(V4_ANY, BindOutcome::InUse)]);
    assert_eq!((status, errors), (CheckStatus::Caution, Vec::new()));
    assert_eq!(detail, ALL_ADDRESSES);

    let site = Site::new();
    let (status, detail, errors) = judge_with(&site, draft_port, &[(V6, BindOutcome::InUse)]);
    assert_eq!((status, errors), (CheckStatus::Caution, Vec::new()));
    assert_eq!(
        detail,
        "Also in use on [::1]:20000; clients that connect to localhost may reach that server"
    );

    let (status, detail, errors) =
        judge_with(&Site::new(), draft_port, &[(V4_ANY, BindOutcome::Denied)]);
    assert_eq!((status, errors), (CheckStatus::Caution, Vec::new()));
    assert_eq!(detail, UNCHECKED);
    ev.pass();
}

#[test]
fn the_other_family_wildcard_is_bound_after_a_free_loopback_and_is_a_loopback_caution() {
    let site = Site::new();

    let (status, detail, errors) = judge_with(&site, 20000, &[(V6_ANY, BindOutcome::InUse)]);

    assert_eq!((status, errors), (CheckStatus::Caution, Vec::new()));
    assert_eq!(
        detail,
        "Also in use on [::1]:20000; clients that connect to localhost may reach that server"
    );
    assert_eq!(
        site.probe.asked().last(),
        Some(&(addr(V6_ANY, 20000), true)),
        "the [::] bind runs after [::1] was free"
    );
}

#[test]
fn an_unavailable_other_loopback_is_unchecked_and_the_other_wildcard_is_not_bound() {
    let site = Site::new();

    let (status, detail, errors) = judge_with(&site, 20000, &[(V6, BindOutcome::Unavailable)]);

    assert_eq!((status, errors), (CheckStatus::Caution, Vec::new()));
    assert_eq!(detail, UNCHECKED);
    assert!(
        !site
            .probe
            .asked()
            .iter()
            .any(|(bound, _)| *bound == addr(V6_ANY, 20000)),
        "no [::] bind after [::1] could not be checked"
    );
}

#[test]
fn a_denied_other_wildcard_after_a_free_loopback_is_unchecked() {
    let site = Site::new();

    let (status, detail, errors) = judge_with(&site, 20000, &[(V6_ANY, BindOutcome::Denied)]);

    assert_eq!((status, errors), (CheckStatus::Caution, Vec::new()));
    assert_eq!(detail, UNCHECKED);
}

#[test]
fn a_holder_found_by_a_later_step_beats_an_earlier_step_that_could_not_be_checked() {
    let ev = evidence!(
        covers = ["VAL-006"],
        kind = Kind::Fixture,
        expected = "the caution text prefers a step 5 holder, then a step 6 holder, over a bind the OS refused",
    );
    // Step 5 refused and step 6 found a holder: the holder is the text.
    let (_, detail, _) = judge_with(
        &Site::new(),
        20000,
        &[(V4_ANY, BindOutcome::Denied), (V6, BindOutcome::InUse)],
    );
    assert_eq!(
        detail,
        "Also in use on [::1]:20000; clients that connect to localhost may reach that server"
    );

    // Step 5 and step 6 both hold the port: step 5 is the text.
    let (_, detail, _) = judge_with(
        &Site::new(),
        20000,
        &[(V4_ANY, BindOutcome::InUse), (V6, BindOutcome::InUse)],
    );
    assert_eq!(detail, ALL_ADDRESSES);

    // Step 5 holds and step 6 was refused: step 5 is the text.
    let (_, detail, _) = judge_with(
        &Site::new(),
        20000,
        &[(V4_ANY, BindOutcome::InUse), (V6, BindOutcome::Denied)],
    );
    assert_eq!(detail, ALL_ADDRESSES);
    ev.pass();
}

#[test]
fn an_ipv6_loopback_endpoint_binds_its_exact_address_wildcard_and_the_ipv4_loopback_and_wildcard() {
    let ev = evidence!(
        covers = ["VAL-006"],
        kind = Kind::Fixture,
        expected = "an endpoint on ::1 is bound on [::1], [::], 127.0.0.1, and 0.0.0.0 with SO_REUSEADDR, and a holder of 0.0.0.0 is a caution on 127.0.0.1",
    );
    let ipv6_host = |dir: &std::path::Path| {
        let manifest = dir.join("provider.yaml");
        let text = std::fs::read_to_string(&manifest).expect("the manifest reads");
        std::fs::write(
            &manifest,
            text.replace("defaultHost: \"127.0.0.1\"", "defaultHost: \"::1\""),
        )
        .expect("the manifest writes");
    };
    let site = Site::with_provider_edit("fake", ipv6_host);
    let mut draft = site.draft("orders");
    draft.endpoints = vec![fixed("http", 20000)];

    let evaluation = site.evaluate(&draft);

    let http = row(&evaluation, "endpoint.http");
    assert_eq!(http.title, "Endpoint ::1:20000");
    assert_eq!(http.status, CheckStatus::Ok, "{http:?}");
    assert_eq!(
        site.probe.asked(),
        [
            (addr(V6, 20000), true),
            (addr(V6_ANY, 20000), true),
            (addr(V4, 20000), true),
            (addr(V4_ANY, 20000), true),
        ]
    );

    let site = Site::with_provider_edit("fake", ipv6_host);
    site.probe
        .answer(addr(V4_ANY, 20000), true, BindOutcome::InUse);
    let evaluation = site.evaluate(&draft);
    let http = row(&evaluation, "endpoint.http");
    assert_eq!(http.status, CheckStatus::Caution);
    assert_eq!(
        http.detail,
        "Also in use on 127.0.0.1:20000; clients that connect to localhost may reach that server"
    );
    ev.pass();
}

#[test]
fn a_port_outside_the_range_is_endpoint_invalid_with_an_out_of_range_field() {
    let site = Site::new();
    let mut draft = site.draft("orders");
    draft.endpoints = vec![fixed("http", 0)];

    let evaluation = site.evaluate(&draft);

    assert_eq!(error_codes(&evaluation), ["instance.endpoint_invalid"]);
    assert_eq!(paths(&evaluation), [("endpoints[0].port", "out_of_range")]);
    assert!(
        site.probe.asked().is_empty(),
        "step 1 stops before any bind"
    );
}

#[test]
fn a_port_a_stopped_instance_holds_conflicts_even_when_nothing_listens_and_a_retained_one_does_not()
{
    let ev = evidence!(
        covers = ["BR-004", "BR-005"],
        kind = Kind::Fixture,
        expected = "a fixed port of an instance that is stopped is supervisor.endpoint_conflict naming that instance, with a suggestion that clears every Stackly instance, and a retained instance does not conflict",
    );
    let site = Site::new();
    let other = site.add_instance(|builder| builder.name("busy"));
    let mut draft = site.draft("orders");
    draft.endpoints = vec![fixed("http", 5432)];

    let evaluation = site.evaluate(&draft);

    assert_eq!(error_codes(&evaluation), ["supervisor.endpoint_conflict"]);
    let http = row(&evaluation, "endpoint.http");
    assert_eq!(http.detail, "In use by busy");
    assert_eq!(http.suggested_port, Some(5433));
    let stackly_core::instance::create::CreateError::EndpointConflict {
        other: found, port, ..
    } = evaluation.first_error().expect("an error").clone()
    else {
        panic!("endpoint conflict");
    };
    assert_eq!((found, port.get()), (other.id(), 5432));
    assert!(
        site.probe.asked().iter().all(|(a, _)| a.port() != 5432),
        "step 3 stops before the binds"
    );

    let site = Site::new();
    site.add_instance(|builder| {
        builder
            .name("archive")
            .lifecycle(InstanceLifecycle::Retained)
    });
    let mut draft = site.draft("orders");
    draft.endpoints = vec![fixed("http", 5432)];
    assert!(site.evaluate(&draft).ready());
    ev.pass();
}

#[test]
fn two_endpoints_of_one_draft_cannot_ask_for_the_same_port() {
    let site = Site::with_provider("nats");
    let mut draft = site.draft("orders");
    draft.endpoints = vec![fixed("client", 20000), fixed("monitoring", 20000)];

    let evaluation = site.evaluate(&draft);

    let codes = error_codes(&evaluation);
    assert!(
        codes
            .iter()
            .filter(|c| **c == "instance.endpoint_invalid")
            .count()
            == 2,
        "{codes:?}"
    );
    assert_eq!(
        paths(&evaluation),
        [
            ("endpoints[0].port", "invalid_value"),
            ("endpoints[1].port", "invalid_value")
        ]
    );
}

#[test]
fn a_suggestion_clears_the_other_ports_of_the_same_draft() {
    let site = Site::with_provider("nats");
    site.probe.answer(addr(V4, 20000), true, BindOutcome::InUse);
    let mut draft = site.draft("orders");
    draft.endpoints = vec![fixed("client", 20000), fixed("monitoring", 20001)];

    let evaluation = site.evaluate(&draft);

    assert_eq!(
        row(&evaluation, "endpoint.client").suggested_port,
        Some(20002),
        "20001 belongs to monitoring in the same draft"
    );
    assert_eq!(
        row(&evaluation, "endpoint.monitoring").status,
        CheckStatus::Ok
    );
}

#[test]
fn the_suggestion_has_no_candidate_past_65535_or_after_sixty_four_taken_ports() {
    let site = Site::new();
    let mut draft = site.draft("orders");
    draft.endpoints = vec![fixed("http", 65535)];
    site.probe.answer(addr(V4, 65535), true, BindOutcome::InUse);
    let evaluation = site.evaluate(&draft);
    assert_eq!(row(&evaluation, "endpoint.http").suggested_port, None);

    let site = Site::new();
    site.probe.answer(addr(V4, 20000), true, BindOutcome::InUse);
    site.probe.block(V4, 20001, 20064, BindOutcome::InUse);
    draft.endpoints = vec![fixed("http", 20000)];
    let evaluation = site.evaluate(&draft);
    assert_eq!(
        row(&evaluation, "endpoint.http").suggested_port,
        None,
        "64 attempts, all taken"
    );

    let site = Site::new();
    site.probe.answer(addr(V4, 20000), true, BindOutcome::InUse);
    site.probe.block(V4, 20001, 20063, BindOutcome::InUse);
    let evaluation = site.evaluate(&draft);
    assert_eq!(
        row(&evaluation, "endpoint.http").suggested_port,
        Some(20064),
        "the 64th candidate is still tried"
    );
}

#[test]
fn a_request_that_names_no_endpoint_a_socket_or_an_endpoint_twice_is_one_more_row() {
    let ev = evidence!(
        covers = ["VAL-004"],
        kind = Kind::Fixture,
        expected = "a port request for an unknown endpoint, a unix endpoint, or an endpoint named twice makes an extra endpoint row with instance.endpoint_invalid and the path endpoints[i].name",
    );
    let site = Site::with_provider("postgresql");
    let mut draft = site.draft("orders");
    draft.endpoints = vec![
        fixed("nope", 20000),
        fixed("socket", 20001),
        fixed("client", 20002),
        auto("client"),
    ];

    let evaluation = site.evaluate(&draft);

    assert!(has_row(&evaluation, "endpoint.request.0"));
    assert!(has_row(&evaluation, "endpoint.request.1"));
    assert!(has_row(&evaluation, "endpoint.request.3"));
    assert_eq!(
        row(&evaluation, "endpoint.request.0").detail,
        "This provider has no endpoint by that name."
    );
    assert_eq!(
        row(&evaluation, "endpoint.request.1").detail,
        "Sockets don’t take a port."
    );
    assert_eq!(
        row(&evaluation, "endpoint.request.3").detail,
        "This endpoint is listed twice."
    );
    assert_eq!(
        paths(&evaluation),
        [
            ("endpoints[0].name", "unknown_value"),
            ("endpoints[1].name", "invalid_value"),
            ("endpoints[3].name", "invalid_value")
        ]
    );
    // The first mention of `client` still gets its own row.
    assert!(has_row(&evaluation, "endpoint.client"));
    ev.pass();
}

#[test]
fn the_stored_unix_socket_name_follows_the_port_of_the_endpoint_it_names() {
    let ev = evidence!(
        covers = ["REQ-010"],
        kind = Kind::Fixture,
        expected = "the stored socket name of a unix endpoint renders ${endpoint.client.port} with the fixed port, or auto when the client endpoint is auto",
    );
    let site = Site::with_provider("postgresql");
    let socket_of = |draft: &stackly_core::instance::create::CreateDraft| {
        let evaluation = site.evaluate(draft);
        assert!(evaluation.ready(), "{:#?}", evaluation.checks);
        let resolved = evaluation.resolved.expect("ready");
        resolved
            .endpoints
            .iter()
            .find_map(|endpoint| match endpoint {
                Endpoint::Unix(unix) => Some(unix.socket().to_string()),
                Endpoint::Ip(_) => None,
            })
            .expect("a unix endpoint")
    };

    let mut draft = site.draft("orders");
    draft.endpoints = vec![fixed("client", 20000)];
    assert_eq!(socket_of(&draft), ".s.PGSQL.20000");
    draft.endpoints = Vec::new();
    assert_eq!(socket_of(&draft), ".s.PGSQL.auto");
    ev.pass();
}
