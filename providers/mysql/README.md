# official/mysql

Runs MySQL 8.4 and 9.7 (spec 0042). Each instance has its own data folder,
made by `mysqld --initialize-insecure` from the matching runtime, a password
protected `root` account, a loopback TCP port, and a private Unix socket.

## Accounts

Init sets the root password and creates the accounts below in the same
`mysqld --initialize-insecure` process, so no server ever runs with a root
account that has no password.

- `root@localhost`, `root@127.0.0.1`, and `root@::1` share the password you
  gave at create (or the one Stackly generated). They are three separate
  accounts in MySQL: `ALTER USER` on one does not change the others, so change
  all three, then update the secret in Stackly.
- `stackly_readiness@localhost` has no password and no privileges beyond
  `USAGE`. Readiness runs `SELECT 1` as this account through `mysql` over the
  instance's private socket. Because `skip_name_resolve` is on, the account
  only matches socket connections, never TCP. Do not drop or lock it: the
  instance would stop becoming ready.

## Parameters Stackly owns

`port`, `bind_address`, `socket`, `datadir`, `tmpdir`, `pid_file`,
`skip_name_resolve`, `secure_file_priv`, `general_log_file`,
`slow_query_log_file`, and `mysqlx` are set on the `mysqld` command line.
`mysqld` starts with `--persisted-globals-load=OFF`, so variables saved with
`SET PERSIST` last only until the next restart; change lasting settings in
Stackly. `my.cnf` in the instance config folder is managed by Stackly: change
its values on the instance's Configuration page. Stackly does not overwrite a
hand edit there: the next start fails with `config.drift_detected` (`config
files changed outside stackly: my.cnf`), the edit stays in the file, and the
instance does not start until you resolve the drift.

The binary log is off by default. Turn on the `Binary log` setting (and
restart) when a change data capture tool needs it.

`versions.json` is generated from `runtimes/inventory.toml` by
`cargo xtask codegen`; never edit it by hand.
