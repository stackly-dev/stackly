//! Two MySQL instances never see each other's data (spec 0042 AC-9, AC-081
//! without restore): each holds one sentinel row in
//! `stackly_sentinel.marker`, and after a stop and a start each one reads
//! only its own row, over TCP and over its socket.

use stackly_testkit::evidence::{Evidence, Kind, evidence};
use uuid::Uuid;

use crate::endpoints::stop;
use crate::harness::{MY84, MY97, Made, My, Mysql, ROOT, Site, Via, create_on, new_password};
use crate::support::Api;

/// One running instance with the means to write and read its sentinel.
pub struct SentinelServer {
    /// The name of the instance, and the `id` of its sentinel row.
    pub name: String,
    /// The instance.
    pub made: Made,
    /// The `mysql` pointed at the instance.
    pub mysql: Mysql,
}

impl SentinelServer {
    fn query(&self, via: Via, sql: &str) -> String {
        let ran = self.mysql.run(via, ROOT, Some(&self.made.password), sql);
        assert!(ran.ok, "instance {}: {}", self.name, ran.stderr);
        ran.stdout
    }

    /// Writes `value` as the one row of this instance.
    pub fn write(&self, value: &str) {
        self.query(
            Via::Socket,
            &format!(
                "CREATE DATABASE IF NOT EXISTS stackly_sentinel; \
                 CREATE TABLE IF NOT EXISTS stackly_sentinel.marker (id varchar(64) PRIMARY KEY, value varchar(64) NOT NULL); \
                 REPLACE INTO stackly_sentinel.marker VALUES ('{}', '{value}')",
                self.name
            ),
        );
    }

    /// Every row of the table through `via`, as `(id, value)`.
    #[must_use]
    pub fn read(&self, via: Via) -> Vec<(String, String)> {
        self.query(
            via,
            "SELECT id, value FROM stackly_sentinel.marker ORDER BY id",
        )
        .lines()
        .map(|line| {
            let (id, value) = line.split_once('\t').expect("two columns");
            (id.to_owned(), value.to_owned())
        })
        .collect()
    }

    /// Asserts that every server in `expected` reads exactly its own row,
    /// over TCP and over its socket.
    pub fn assert_isolated(expected: &[(&Self, &str)]) {
        for (server, value) in expected {
            for via in [Via::Tcp, Via::Socket] {
                assert_eq!(
                    server.read(via),
                    [(server.name.clone(), (*value).to_owned())],
                    "instance {} over {via:?}",
                    server.name
                );
            }
        }
    }
}

/// Creates and starts `name` on `runtime_id` and wraps it, reached with
/// the `mysql` of `runtime_dir`.
async fn running(
    site: &Site,
    api: &Api,
    runtime_id: Uuid,
    runtime_dir: &std::path::Path,
    name: &str,
) -> SentinelServer {
    let made = create_on(api, runtime_id, name).await;
    api.start_ready(made.id).await;
    SentinelServer {
        name: name.to_owned(),
        mysql: site.mysql_in(runtime_dir, made.id, made.port),
        made,
    }
}

/// Stops and starts every server, one after the other.
async fn restart_all(api: &Api, servers: &[&SentinelServer]) {
    for server in servers {
        stop(api, server.made.id).await;
    }
    for server in servers {
        api.start_ready(server.made.id).await;
    }
}

async fn two_instances_of_one_line_read_only_their_own_sentinel(my: My, ev: Evidence) {
    let Some(site) = Site::open(&ev, my) else {
        return;
    };
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;
    let line = my.line.replace('.', "");
    let a = running(
        &site,
        &api,
        site.runtime_id,
        &site.runtime_dir,
        &format!("a{line}"),
    )
    .await;
    let b = running(
        &site,
        &api,
        site.runtime_id,
        &site.runtime_dir,
        &format!("b{line}"),
    )
    .await;
    assert_ne!(a.made.port, b.made.port);
    let version = a.mysql.scalar(&a.made.password, "SELECT VERSION()");
    site.subject(&ev, &version);
    let (va, vb) = (new_password(), new_password());
    a.write(&va);
    b.write(&vb);
    SentinelServer::assert_isolated(&[(&a, &va), (&b, &vb)]);

    restart_all(&api, &[&a, &b]).await;
    SentinelServer::assert_isolated(&[(&a, &va), (&b, &vb)]);

    stop(&api, a.made.id).await;
    stop(&api, b.made.id).await;
    daemon.shutdown().await;
    ev.pass_with("two instances of one line each read only their own sentinel over TCP and the socket, before and after a restart");
}

// covers: AC-9 (spec 0042)
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed MySQL 8.4 artifact in the runtime cache"]
async fn mysql_8_4_two_instances_read_only_their_own_sentinel() {
    let ev = evidence!(
        covers = ["REQ-055", "AC-081"],
        kind = Kind::Native,
        expected = "two MySQL 8.4 instances with their own data folder and port each hold one sentinel row in stackly_sentinel.marker, and after both stop and start again each reads only its own row over TCP and over its socket",
    );
    two_instances_of_one_line_read_only_their_own_sentinel(MY84, ev).await;
}

// covers: AC-9 (spec 0042)
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed MySQL 9.7 artifact in the runtime cache"]
async fn mysql_9_7_two_instances_read_only_their_own_sentinel() {
    let ev = evidence!(
        covers = ["REQ-055", "AC-081"],
        kind = Kind::Native,
        expected = "two MySQL 9.7 instances with their own data folder and port each hold one sentinel row in stackly_sentinel.marker, and after both stop and start again each reads only its own row over TCP and over its socket",
    );
    two_instances_of_one_line_read_only_their_own_sentinel(MY97, ev).await;
}

// covers: AC-9 (spec 0042)
#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs both managed MySQL artifacts in the runtime cache"]
async fn mysql_8_4_and_9_7_side_by_side_read_only_their_own_sentinel() {
    let ev = evidence!(
        covers = ["REQ-055", "AC-081"],
        kind = Kind::Native,
        expected = "one MySQL 8.4 and one MySQL 9.7 instance run side by side, each holds one sentinel row, and after both stop and start again each reads only its own row over TCP and over its socket",
    );
    let Some(site) = Site::open(&ev, MY84) else {
        return;
    };
    let Some((other, _)) = site.install_other(&ev, MY97) else {
        return;
    };
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;
    let other_dir = crate::harness::runtime_dir_of(&api, other.as_uuid()).await;
    let a = running(&site, &api, site.runtime_id, &site.runtime_dir, "side84").await;
    let b = running(&site, &api, other.as_uuid(), &other_dir, "side97").await;
    assert_eq!(
        a.mysql.scalar(&a.made.password, "SELECT VERSION()"),
        MY84.version
    );
    let version = b.mysql.scalar(&b.made.password, "SELECT VERSION()");
    assert_eq!(version, MY97.version);
    site.subject(&ev, MY84.version);
    let (va, vb) = (new_password(), new_password());
    a.write(&va);
    b.write(&vb);
    SentinelServer::assert_isolated(&[(&a, &va), (&b, &vb)]);
    restart_all(&api, &[&a, &b]).await;
    SentinelServer::assert_isolated(&[(&a, &va), (&b, &vb)]);

    stop(&api, a.made.id).await;
    stop(&api, b.made.id).await;
    daemon.shutdown().await;
    ev.pass_with(&format!(
        "MySQL {} and {version} side by side each read only their own sentinel",
        MY84.version
    ));
}
