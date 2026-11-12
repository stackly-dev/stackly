# official/clickhouse

Runs one single node ClickHouse server per instance (spec 0043), with two
endpoints on loopback and a password protected `default` user. Every path
the server writes stays inside the instance folder.

## Endpoints

- `http` (default port 8123): the HTTP interface, `/ping`, and the Play UI
  at `/play`.
- `native` (default port 9000): the native TCP protocol that
  `clickhouse client` and most drivers use.

Both listen only on `127.0.0.1`. The rendered `config.xml` writes no other
port, so MySQL, PostgreSQL, gRPC, HTTPS, interserver, Prometheus, and Keeper
listeners do not exist. Cluster, replication, Keeper, and distributed DDL
are off, and so are the system log tables (`query_log` and the others).

## Users

- `default` signs in with the instance's password, over HTTP and native TCP,
  from loopback only. ClickHouse only keeps a SHA-256 hash of the password,
  in `users.xml` in the instance's secrets folder, which is written again
  before every start and reload and removed after a clean stop. ClickHouse
  also copies that file, hash included, to `data/preprocessed_configs/`.
- `default` has access management, so you can create other users and roles
  with SQL (`CREATE USER ...`). ClickHouse stores them in `data/access/`.
- `stackly_readiness` has no password and may only `SELECT` from
  `system.one`. Stackly uses it to check that the server that answers is
  this instance.
- `stackly_reload` has no password and may only run `SYSTEM RELOAD CONFIG`
  and `SYSTEM RELOAD USERS`. Stackly uses it to reload the instance.

Leave the two `stackly_` users as they are: Stackly writes them again on
every start and reload, and readiness and reload stop working without them.

## Reload

Changing the log level or the password and reloading renders `config.xml`
and `users.xml` again, then runs `SYSTEM RELOAD CONFIG` as `stackly_reload`.
The server keeps its process; a new password works as soon as the reload
ends.

## What Stackly does not manage

ClickHouse also merges files in `config.d/` and `conf.d/` next to
`config.xml`, and `users.d/` next to `users.xml`, and it reloads a changed
`config.xml` on its own while it runs. Stackly never creates those folders
and never looks at them. A file you put there can open a listener that
Stackly does not show.

## Resources

An idle instance uses about 0.5 GB of RAM and about 170 threads. The runtime
is a single 892 MB binary.

## Windows

There is no native Windows build. Upstream runs ClickHouse on Windows only
through WSL, which Stackly never installs or uses.

`versions.json` is generated from `runtimes/inventory.toml` by
`cargo xtask codegen`; never edit it by hand.
