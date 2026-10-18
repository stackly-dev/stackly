# official/nats

Runs `nats-server` with one rendered `nats.conf` per instance (spec 0009):
the client listener and the HTTP monitoring listener on loopback, and
JetStream storing into the instance's data folder. Readiness is an HTTP probe
to `/healthz` on the monitoring endpoint until feature 23 adds the native
`nats-readiness` probe.

`versions.json` is generated from `runtimes/inventory.toml` by
`cargo xtask codegen`; never edit it by hand.
