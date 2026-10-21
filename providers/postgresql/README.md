# official/postgresql

Runs PostgreSQL 17 and 18 (spec 0019). Each instance is its own cluster made
by `initdb` from the matching runtime, with a password protected `postgres`
superuser (SCRAM, never `trust`), a loopback TCP port, and a private Unix
socket.

## Readiness role

`stackly_readiness` is created during init. Readiness runs `SELECT 1` as that
role through `psql` over the instance's private socket, using peer
authentication (the socket folder is mode 0700). Do not drop or change the
role: the instance would stop becoming ready.

## Parameters Stackly owns

`port`, `listen_addresses`, `unix_socket_directories`,
`unix_socket_permissions`, `config_file`, `hba_file`, `ident_file`,
`logging_collector`, and `log_destination` are set on the `postgres` command
line, so `ALTER SYSTEM` cannot move them. `postgresql.conf`, `pg_hba.conf`,
and `pg_ident.conf` in the instance config folder are written again before
every start and reload; hand edits there are lost. `ALTER SYSTEM` still works
for every other parameter.

`versions.json` is generated from `runtimes/inventory.toml` by
`cargo xtask codegen`; never edit it by hand.
