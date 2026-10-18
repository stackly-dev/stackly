# official/traefik

Runs `traefik` with a rendered static config (`traefik.yaml`) and a dynamic
file (`dynamic/routes.yaml`) per instance (spec 0009). The `web` entry point
serves the routes, and a separate loopback `ping` entry point answers the
readiness probe at `/ping`. The dashboard and the insecure API stay off.

Routes come from the `routes` setting. Changing them reloads by rewriting
`dynamic/routes.yaml`; Traefik watches that folder, so no signal is sent.

`versions.json` is generated from `runtimes/inventory.toml` by
`cargo xtask codegen`; never edit it by hand.
