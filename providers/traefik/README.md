# official/traefik

A local HTTP proxy per project (spec 0041). Each instance runs its own
`traefik` process with a rendered static config (`traefik.yaml`) and one
watched dynamic file (`dynamic/routes.yaml`), so a route you change on one
instance never touches another.

## Endpoints

- `web` (client, `127.0.0.1`): serves your routes, and nothing else. A
  request no route matches answers 404.
- `ping` (monitoring, `127.0.0.1`): answers `GET /ping` and the read only
  Traefik API, for example `GET /api/http/routers`. Stackly's readiness probe
  reads it. Any local process can read router names, rules, and backend URLs
  here; nothing secret lives there, and the API accepts no writes.

Both listeners bind only to the loopback, on the ports in the registry.

## Changing routes

Routes come from the `routes` setting: a list of `name`, `rule`, and
`backendUrl`, at most 64, each `name` unique in the instance. Change them in
the Configuration tab, or with
`stackly instance config apply <instance> --set-json routes=<json>`. An apply
rewrites `dynamic/routes.yaml`; Traefik watches that folder, so the change is
a reload with no restart and no signal. Changing `logLevel` restarts the
instance.

A value the schema refuses (a repeated name, an empty rule, a backend that is
not `http://` on the loopback, more than 64 routes) is refused before anything
is written.

## When the instance says `routes_mismatch`

The instance is ready only when the `@file` routers Traefik loaded match
`dynamic/routes.yaml` exactly. `routes_mismatch` means they differ: usually
Traefik rejected a rule it can't parse. The apply fails, the last revision
that was ready stays as it was, and Stackly never rolls back on its own. Fix
the rule and apply again, or roll back to the last ready revision. The Logs
tab shows the router Traefik rejected and why.

## The `dynamic` folder

`config/dynamic/` holds only the file Stackly writes and watches. Traefik
also loads any other file you put there, and a file that holds a router makes
the instance `routes_mismatch` until you move it out. Stackly never reads,
moves, or deletes such a file.

## Limits of this revision

- HTTP only. HTTPS is not offered: there is no certificate setting and a
  backend must be `http://`.
- The Traefik dashboard is always off.
- Stackly never edits `/etc/hosts`, DNS, or the system trust store; use a host
  name that already resolves to this Mac (for example `*.localhost`), or send
  the `Host` header yourself.

`versions.json` is generated from `runtimes/inventory.toml` by
`cargo xtask codegen`; never edit it by hand.
