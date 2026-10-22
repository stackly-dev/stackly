//! The sentinel helper of the native PostgreSQL tests (spec 0024, step 8):
//! one table row per cluster that proves a cluster reads its own data and
//! nobody else's. Spec 0025 reuses it for its three cluster matrix.
//!
//! Every query goes over TCP `127.0.0.1:<port>` with the password in the
//! `PGPASSWORD` of the child `psql` only, so each read also proves that the
//! port belongs to the cluster the test thinks it does. A failure panics
//! with the name of the cluster and the stderr of `psql`; the password is
//! never printed.
#![allow(
    dead_code,
    reason = "spec 0025 and each scenario module use a part of this helper"
)]

use stackly_testkit::evidence::{Kind, evidence};
use uuid::Uuid;

use crate::harness::{PG17, Psql, SUPERUSER, Site, Via, create_instance, stop_and_shutdown};

/// The table every sentinel lives in.
const TABLE: &str = "stackly_sentinel";

/// One running cluster with the means to write and read its sentinel.
#[derive(Debug, Clone)]
pub struct SentinelCluster {
    /// The name of the instance, and the `id` of its sentinel row.
    pub name: String,
    /// The instance ID.
    pub id: Uuid,
    /// The `psql` of the runtime of this instance, pointed at this instance.
    pub psql: Psql,
    /// The password of the superuser the test holds.
    pub password: String,
}

/// `value` as a SQL string literal.
fn literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

impl SentinelCluster {
    /// A cluster `name` of the instance `id`, reached with `psql` and the
    /// superuser `password`.
    #[must_use]
    pub fn new(name: &str, id: Uuid, psql: Psql, password: &str) -> Self {
        Self {
            name: name.to_owned(),
            id,
            psql,
            password: password.to_owned(),
        }
    }

    /// Runs `sql` as the superuser over TCP and returns its output.
    fn query(&self, sql: &str) -> String {
        let ran = self
            .psql
            .run(Via::Tcp, SUPERUSER, Some(&self.password), "postgres", sql);
        assert!(ran.ok, "cluster {}: psql failed: {}", self.name, ran.stderr);
        ran.stdout
    }

    /// Writes `value` as the one row of this cluster: creates the table when
    /// it is missing, then upserts the row whose `id` is the cluster name.
    pub fn write(&self, value: &str) {
        self.query(&format!(
            "CREATE TABLE IF NOT EXISTS {TABLE} (id text PRIMARY KEY, value text NOT NULL)"
        ));
        self.query(&format!(
            "INSERT INTO {TABLE} (id, value) VALUES ({}, {}) \
             ON CONFLICT (id) DO UPDATE SET value = EXCLUDED.value",
            literal(&self.name),
            literal(value)
        ));
    }

    /// Every row of the table ordered by `id`; a table that does not exist
    /// reads as no rows.
    #[must_use]
    pub fn read(&self) -> Vec<(String, String)> {
        if self.query(&format!("SELECT to_regclass('{TABLE}') IS NULL")) == "t" {
            return Vec::new();
        }
        let json = self.query(&format!(
            "SELECT coalesce(json_agg(json_build_array(id, value) ORDER BY id), '[]'::json) \
             FROM {TABLE}"
        ));
        serde_json::from_str(&json)
            .unwrap_or_else(|error| panic!("cluster {}: the rows are not JSON: {error}", self.name))
    }

    /// The `system_identifier` of the cluster, unique per `initdb`.
    #[must_use]
    pub fn system_identifier(&self) -> String {
        let identifier = self.query("SELECT system_identifier FROM pg_control_system()");
        assert!(
            !identifier.is_empty() && identifier.bytes().all(|byte| byte.is_ascii_digit()),
            "cluster {}: not a system identifier",
            self.name
        );
        identifier
    }

    /// Asserts that every cluster in `expected` reads exactly one row, its
    /// own, with the value given.
    pub fn assert_isolated(expected: &[(&SentinelCluster, &str)]) {
        for (cluster, value) in expected {
            let rows = cluster.read();
            assert_eq!(
                rows.len(),
                1,
                "cluster {} reads {} sentinel rows, not one of its own",
                cluster.name,
                rows.len()
            );
            assert_eq!(
                rows[0].0, cluster.name,
                "cluster {} reads the row of another cluster",
                cluster.name
            );
            assert_eq!(
                rows[0].1, *value,
                "cluster {} reads another value than it wrote",
                cluster.name
            );
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "native: needs the managed PostgreSQL 17 artifact in the runtime cache"]
async fn the_sentinel_helper_writes_reads_and_names_one_cluster() {
    let ev = evidence!(
        covers = ["REQ-009"],
        kind = Kind::Native,
        expected = "a PostgreSQL 17 instance reads no sentinel before a write, one row of its own after it, the new value after a second write, and a numeric system identifier that does not change",
    );
    let Some(site) = Site::open(&ev, PG17) else {
        return;
    };
    let daemon = site.in_process(|_| {}).await;
    let api = site.api().await;
    let made = create_instance(&site, &api, "pg17-sentinel").await;
    api.start_ready(made.id).await;
    let cluster = SentinelCluster::new(
        "pg17-sentinel",
        made.id,
        site.psql(made.id, made.port),
        &made.password,
    );
    let version = cluster.psql.scalar(&made.password, "SHOW server_version");
    site.subject(&ev, &version);

    assert!(cluster.read().is_empty(), "a new cluster has no sentinel");
    let identifier = cluster.system_identifier();
    cluster.write("first");
    SentinelCluster::assert_isolated(&[(&cluster, "first")]);
    cluster.write("second");
    SentinelCluster::assert_isolated(&[(&cluster, "second")]);
    assert_eq!(cluster.system_identifier(), identifier);

    stop_and_shutdown(&api, daemon, made.id).await;
    ev.pass_with(&format!(
        "PostgreSQL {version} held no sentinel, then one row of its own, then its second value"
    ));
}
