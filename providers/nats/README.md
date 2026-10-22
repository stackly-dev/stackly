# official/nats

Runs `nats-server` with one rendered `nats.conf` per instance (spec 0009):
the client listener and the HTTP monitoring listener on loopback, and
JetStream storing into the instance's data folder (`<data>/jetstream`).

## Readiness

Readiness uses the native `nats-readiness` probe (spec 0023), which speaks
the NATS client protocol to the `client` endpoint instead of trusting an open
port. One attempt checks that:

- the server that answers reports the `server_name` of this instance
  (`serverName` is the instance name), so another process on the port is
  never taken for this one;
- JetStream is on or off as the instance's `jetstream` setting asks;
- the server answers a `PING` with `PONG`;
- when JetStream is on, the `$JS.API.INFO` API answers without an error.

The probe shows up in the monitoring page as a client named
`stackly-readiness` that connects about every 10 seconds, sends no
credentials, and closes the connection when the attempt ends.

## Limits of this version

- The instance has no authentication yet. Any local process can connect to
  the client and monitoring ports on loopback, as with the upstream default.
  Credentials come with feature 39.
- The monitoring listener is bound to loopback only.

## Connection

The `connection` block lists the client URL (`nats://<address>`) and the
monitoring URL (`http://<address>`), both without credentials.

`versions.json` is generated from `runtimes/inventory.toml` by
`cargo xtask codegen`; never edit it by hand.
