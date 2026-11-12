//! A hand edit of `my.cnf` is drift (spec 0042 *Template config* and its
//! correction, spec 0029): the rendered file says so in three comment lines,
//! the next start is refused with `config.drift_detected` before any
//! `mysqld` runs, the edit stays byte for byte however often the start is
//! tried, and keeping the revision writes the rendered copy back.

use stackly_protocol::ErrorBody;
use stackly_protocol::instance::InstanceLifecycle;
use stackly_testkit::evidence::{Evidence, Kind, evidence};
use uuid::Uuid;

use crate::config_calls::Cfg;
use crate::endpoints::stop;
use crate::harness::{
    MY84, My, Site, create_instance, mysqld_serving, stop_and_shutdown, variable,
};
use crate::support::Api;

/// The comment lines the corrected template writes (spec 0042, commit
/// `a046da9f`).
const DRIFT_COMMENT: &str = "# Managed by Stackly. Change these values on the instance's Configuration page.\n\
# Stackly does not overwrite an edit made here: it reports the file as changed\n\
# and the instance does not start until you resolve it.\n";

/// The code of a refused start, whether the daemon refuses the request or
/// the operation it began fails.
async fn refused_start_code(api: &Api, id: Uuid) -> String {
    match api.start(id).await {
        Err(error) => error.code().to_owned(),
        Ok(reply) => {
            let accepted = reply
                .accepted
                .expect("a start from stopped begins an operation");
            let failed = api.wait_final(accepted.operation.id).await;
            let error: ErrorBody = failed
                .operation
                .failure
                .expect("a refused start fails")
                .error;
            error.code().to_owned()
        }
    }
}

async fn refuses_to_start_on_a_hand_edited_my_cnf_and_keeps_the_edit(my: My, ev: Evidence) {
    let Some(site) = Site::open(&ev, my) else {
        return;
    };
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;
    let cfg = Cfg { api: &api };
    let made = create_instance(&site, &api, "drift").await;
    api.start_ready(made.id).await;
    let mysql = site.mysql(made.id, made.port);
    let version = mysql.scalar(&made.password, "SELECT VERSION()");
    site.subject(&ev, &version);
    cfg.wait_view(made.id, "a last known good", |view| {
        view.last_known_good.is_some()
    })
    .await;
    stop(&api, made.id).await;

    let path = site.instance_paths(made.id).config.join("my.cnf");
    let rendered = std::fs::read(&path).expect("my.cnf was rendered");
    let text = String::from_utf8(rendered.clone()).expect("my.cnf is UTF-8");
    assert!(text.contains(DRIFT_COMMENT), "{text}");
    assert!(!text.contains("lost on the next start"), "{text}");

    let mut edited = rendered.clone();
    edited.extend_from_slice(b"# edited by hand\nmax_connections = 9\n");
    std::fs::write(&path, &edited).expect("my.cnf is edited by hand");

    for attempt in 1..=2 {
        assert_eq!(
            refused_start_code(&api, made.id).await,
            "config.drift_detected",
            "start attempt {attempt}"
        );
        assert_eq!(
            std::fs::read(&path).expect("my.cnf can be read"),
            edited,
            "start attempt {attempt} changed the hand edit"
        );
        assert!(
            mysqld_serving(&site.instance_paths(made.id).data).is_empty(),
            "start attempt {attempt} spawned a mysqld"
        );
        assert_ne!(
            api.get(made.id).await.expect("the instance").lifecycle,
            InstanceLifecycle::Running
        );
    }
    let drift = cfg.view(made.id).await.drift.expect("the edit is drift");
    assert!(
        drift.files.iter().any(|file| file.path == "my.cnf"),
        "{drift:?}"
    );

    let kept = cfg
        .keep(made.id)
        .await
        .expect("keep revision is accepted")
        .accepted
        .expect("a drift begins an operation");
    api.wait_final(kept.operation.id).await;
    assert_eq!(
        std::fs::read(&path).expect("my.cnf can be read"),
        rendered,
        "keeping the revision writes the rendered copy back"
    );
    api.start_ready(made.id).await;
    assert_eq!(variable(&mysql, &made.password, "max_connections"), "151");

    stop_and_shutdown(&api, daemon, made.id).await;
    ev.pass_with("my.cnf names drift in three comment lines, a hand edit refused two starts with config.drift_detected and stayed byte for byte, keep revision restored the rendered file");
}

// covers: AC-1 (spec 0042)
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed MySQL 8.4 artifact in the runtime cache"]
async fn mysql_8_4_refuses_to_start_on_a_hand_edited_my_cnf_and_keeps_the_edit() {
    let ev = evidence!(
        covers = ["REQ-055"],
        kind = Kind::Native,
        expected = "the rendered my.cnf holds the three comment lines that call a hand edit drift and not the words lost on the next start; after a hand edit two starts are refused with config.drift_detected, no mysqld runs, the edit stays byte for byte, and keep revision writes the rendered file back so the next start is ready with max_connections 151",
    );
    refuses_to_start_on_a_hand_edited_my_cnf_and_keeps_the_edit(MY84, ev).await;
}
