//! The Open In actions of a MySQL instance (spec 0042 AC-18, spec 0070):
//! TablePlus, Sequel Ace, and `mysql` in Terminal, in manifest order, none
//! with a secret value, and `mysql` from the runtime of the instance.

use stackly_protocol::instance::{ConnectionAvailability, OpenActionKind};
use stackly_testkit::evidence::{Evidence, Kind, evidence};

use crate::harness::{MY84, My, PASSWORD_FIELD, Site, create_instance, stop_and_shutdown};

async fn shows_three_open_actions_with_no_secret_and_the_runtime_mysql(my: My, ev: Evidence) {
    let Some(site) = Site::open(&ev, my) else {
        return;
    };
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;
    let made = create_instance(&site, &api, "opened").await;
    api.start_ready(made.id).await;
    let mysql = site.mysql(made.id, made.port);
    let version = mysql.scalar(&made.password, "SELECT VERSION()");
    site.subject(&ev, &version);

    let detail = api.detail(made.id).await.expect("the detail can be read");
    let names: Vec<&str> = detail
        .open_actions
        .iter()
        .map(|action| action.name.as_str())
        .collect();
    assert_eq!(names, ["tableplus", "sequel_ace", "mysql"]);
    let text = serde_json::to_string(&detail.open_actions).expect("the actions serialize");
    assert!(
        !text.contains(&made.password),
        "an action holds the password"
    );
    for action in &detail.open_actions {
        let secret = action
            .copy_secret
            .as_ref()
            .unwrap_or_else(|| panic!("{} copies the root password", action.name));
        assert_eq!(secret.field, PASSWORD_FIELD);
    }
    for (name, bundle) in [
        ("tableplus", "com.tinyapp.TablePlus"),
        ("sequel_ace", "com.sequel-ace.sequel-ace"),
    ] {
        let action = detail
            .open_actions
            .iter()
            .find(|action| action.name == name)
            .expect("the app action");
        assert_eq!(action.kind, OpenActionKind::App);
        assert_eq!(action.app_bundle_ids, [bundle]);
        if action.state == ConnectionAvailability::Available {
            assert_eq!(
                action.url.as_deref(),
                Some(format!("mysql://root@127.0.0.1:{}/", made.port).as_str())
            );
        }
    }
    let terminal = detail
        .open_actions
        .iter()
        .find(|action| action.name == "mysql")
        .expect("the mysql action");
    assert_eq!(terminal.kind, OpenActionKind::Terminal);
    assert_eq!(terminal.state, ConnectionAvailability::Available);
    let command = terminal.command.as_ref().expect("an available command");
    assert_eq!(
        command.executable,
        site.bin("mysql").display().to_string(),
        "the runtime's mysql"
    );
    assert_eq!(
        command.args,
        [
            "--no-defaults".to_owned(),
            "--protocol=socket".to_owned(),
            format!("--socket={}", mysql.socket().display()),
            "--user=root".to_owned(),
            "--password".to_owned(),
        ]
    );

    stop_and_shutdown(&api, daemon, made.id).await;
    ev.pass_with("tableplus, sequel_ace, and mysql, no secret value, mysql from the runtime");
}

// covers: AC-18 (spec 0042)
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed MySQL 8.4 artifact in the runtime cache"]
async fn mysql_8_4_shows_three_open_actions_with_no_secret_and_the_runtime_mysql() {
    let ev = evidence!(
        covers = ["REQ-055"],
        kind = Kind::Native,
        expected = "instance.get_detail of a ready MySQL 8.4 instance lists the open actions tableplus, sequel_ace, and mysql in that order, each copying rootPassword by reference and none holding its value, and the mysql action runs bin/mysql of the instance runtime over the instance socket",
    );
    shows_three_open_actions_with_no_secret_and_the_runtime_mysql(MY84, ev).await;
}
