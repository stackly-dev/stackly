# official/traefik

Runs `traefik` with a rendered static config (`traefik.yaml`) and a dynamic
file (`dynamic/routes.yaml`) per instance (spec 0009). The `web` entry point
serves the routes, and a separate loopback entry point named `traefik` (the
`ping` endpoint) answers `/ping` and the read only API. The dashboard stays
off. Readiness uses the native `traefik-routes` probe (spec 0009 AC-22): ready
only when `/ping` answers 200 and the `@file` routers Traefik loaded match
`dynamic/routes.yaml` exactly, so a reload with a rule Traefik rejects fails
its verify.

Routes come from the `routes` setting. Changing them reloads by rewriting
`dynamic/routes.yaml`; Traefik watches that folder, so no signal is sent.

`versions.json` is generated from `runtimes/inventory.toml` by
`cargo xtask codegen`; never edit it by hand.
