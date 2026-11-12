# official/valkey

Runs `valkey-server` as one standalone server per instance (spec 0040):
always with a password, listening only on the loopback `client` endpoint,
and with one persistence mode you pick once when you create the instance.

## Persistence modes

The `persistence` setting is `rdb`, `aof`, or `volatile`. It is set once at
create. To change the mode, create a new instance and move the data
yourself.

| Mode | On disk | Normal stop, then start | Process crash, then respawn | Host power loss (not tested) |
| --- | --- | --- | --- | --- |
| `rdb` | `dump.rdb` | all data comes back (the last snapshot is written at stop) | data up to the last snapshot; later writes are lost | same as a process crash |
| `aof` | `appendonlydir/` (manifest, base, incr) | all data comes back | every write Valkey accepted comes back | up to about the last second can be lost |
| `volatile` | nothing | all data is lost | all data is lost | all data is lost |

A planned daemon restart also stops and starts Valkey instances until the
Valkey handover is proven, so a `volatile` instance loses its data there too.

## What Stackly owns

The port, the bind address, the data folder, and the persistence mode are
set on the `valkey-server` command line, and the server runs without a
config file argument: the three rendered files are loaded with `--include`.
So `CONFIG REWRITE` and `ACL SAVE` are refused, and `CONFIG SET dir` and
`CONFIG SET dbfilename` are refused as protected configs.

`CONFIG SET` still works for the parameters Valkey can change while it runs,
and it lasts only until the process stops: the next start goes back to the
command line and to the instance's settings. The one exception is a file you
write yourself. A snapshot you write with `SAVE`, `BGSAVE`, or a save point
set through `CONFIG SET` is loaded again at the next start, also in
`volatile` mode, until you delete `dump.rdb` while the instance is stopped.

## Users

- `default` signs in with the instance's password. The server only keeps a
  SHA-256 hash of it, in `users.conf` in the instance's secrets folder, which
  is written again before every start and removed after a clean stop.
- `stackly-readiness` has no password and may run `PING` and nothing else.
  Stackly uses it to check that the instance is ready, and writes it again
  before every start.
- Users you add with `ACL SETUSER` last until the next restart, because
  Valkey writes no ACL file.

## Readiness

Readiness runs the runtime's own `valkey-cli` and sends `PING` as
`stackly-readiness`. Ready means Valkey answers `PONG` after the data is
loaded: while Valkey loads a dataset it answers `LOADING`, and the instance
stays not ready with that message.

## Connection

The `connection` block lists a `redis://` URL and a `valkey://` URL, both for
the `default` user. `redis://` is the scheme most client libraries take; the
server is still Valkey.

`versions.json` is generated from `runtimes/inventory.toml` by
`cargo xtask codegen`; never edit it by hand.
