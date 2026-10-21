//! Where the postmaster listens and which settings are Stackly's (spec 0019
//! AC-8).

use std::os::unix::fs::PermissionsExt;

use stackly_protocol::instance::EndpointPortMode;
use stackly_testkit::evidence::{Kind, evidence};

use crate::harness::{PG18, Site, create_auto, create_instance, output_of, stop_and_shutdown};

fn mode_of(path: &std::path::Path) -> u32 {
    std::fs::metadata(path)
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()))
        .permissions()
        .mode()
        & 0o777
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed PostgreSQL 18 artifact in the runtime cache"]
async fn postgresql_18_listens_only_on_its_endpoint_and_keeps_the_settings_stackly_owns() {
    let ev = evidence!(
        covers = ["REQ-035", "AC-035"],
        kind = Kind::Native,
        expected = "the postmaster listens only on 127.0.0.1 at the endpoint port and on one private socket in a 0700 folder, ALTER SYSTEM SET port does not move it, and an Auto port is stored and named by the socket",
    );
    let Some(site) = Site::open(&ev, PG18) else {
        return;
    };
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;
    let made = create_instance(&site, &api, "listen").await;
    let running = api.start_ready(made.id).await;
    let pid = running.process.as_ref().expect("a running process").pid;
    let run = site.run_dir(made.id);
    let socket = run.join(format!(".s.PGSQL.{}", made.port));
    let psql = site.psql(made.id, made.port);
    let version = psql.scalar(&made.password, "SHOW server_version");
    site.subject(&ev, &version);

    // `-i` and `-U` are ANDed by `-a`, so each kind is listed on its own.
    let pid_text = pid.to_string();
    let tcp = output_of("/usr/sbin/lsof", &["-a", "-p", &pid_text, "-i", "-n", "-P"]);
    let unix = output_of("/usr/sbin/lsof", &["-a", "-p", &pid_text, "-U", "-n", "-P"]);
    let lsof = format!("{tcp}\n{unix}");
    let listening: Vec<&str> = lsof.lines().filter(|l| l.contains("(LISTEN)")).collect();
    assert_eq!(listening.len(), 1, "{lsof}");
    assert!(
        listening[0].contains(&format!("127.0.0.1:{}", made.port)),
        "{lsof}"
    );
    let tcp_other: Vec<&str> = lsof
        .lines()
        .filter(|l| l.contains(" TCP ") && !l.contains("->") && !l.contains("(LISTEN)"))
        .collect();
    assert!(tcp_other.is_empty(), "{lsof}");
    let sockets: Vec<&str> = lsof
        .lines()
        .filter_map(|l| l.split_whitespace().last())
        .filter(|name| name.starts_with('/'))
        .collect();
    assert!(!sockets.is_empty(), "{lsof}");
    assert!(
        sockets
            .iter()
            .all(|name| *name == socket.to_str().unwrap_or_default()),
        "{sockets:?} against {}",
        socket.display()
    );
    assert_eq!(mode_of(&socket), 0o700);
    assert_eq!(mode_of(&run), 0o700);
    assert!(run.join(format!(".s.PGSQL.{}.lock", made.port)).exists());

    // A parameter on the command line beats postgresql.auto.conf.
    let other = made.port.wrapping_add(1).max(1025);
    let ran = psql.admin(&made.password, &format!("ALTER SYSTEM SET port = {other}"));
    assert!(ran.ok, "{ran:?}");
    let accepted = api.stop_when_free(made.id).await.accepted.expect("a stop");
    api.wait_state(
        accepted.operation.id,
        stackly_protocol::operation::OperationState::Succeeded,
    )
    .await;
    api.start_ready(made.id).await;
    assert_eq!(
        psql.scalar(&made.password, "SHOW port"),
        made.port.to_string()
    );

    // An Auto port is stored, and the socket follows it.
    let (auto, password) = create_auto(&site, &api, "auto").await;
    let summary = api.start_ready(auto).await;
    let client = &summary.endpoints[0];
    let port = client.port.expect("the bound Auto port is shown");
    assert_eq!(client.port_mode, EndpointPortMode::Auto);
    assert!(site.run_dir(auto).join(format!(".s.PGSQL.{port}")).exists());
    let auto_psql = site.psql(auto, port);
    assert_eq!(auto_psql.scalar(&password, "SHOW port"), port.to_string());
    let accepted = api.stop_when_free(auto).await.accepted.expect("a stop");
    api.wait_state(
        accepted.operation.id,
        stackly_protocol::operation::OperationState::Succeeded,
    )
    .await;

    stop_and_shutdown(&api, daemon, made.id).await;
    ev.pass_with("one TCP listener on 127.0.0.1 and one 0700 socket, port fixed by argv, Auto port named the socket");
}
