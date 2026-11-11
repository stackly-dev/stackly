# Example provider

`testing/example` is the smallest complete provider package. It runs
`stackly-fake-server` (`crates/fake-server`), so it needs no runtime download,
and its namespace is `testing`, so it never ships in the app catalog.
`cargo xtask provider new <name>` copies this folder into a new package
`testing/<name>`, so every file here is also the starting point of a new
provider. The provider guide is `docs/providers/README.md`.

## Files

| File | What it holds |
| --- | --- |
| `provider.yaml` | The manifest (`stackly/v1alpha1`): identity, the `http` endpoint, the `serverConfig` file, the init, start, stop, and reload steps, the readiness and health probes, the capabilities, and one connection entry |
| `schemas/settings.schema.json` | The settings form: `greeting` (applied on reload), `readyDelayMs` (applied on restart, advanced), and the secret `authToken` |
| `templates/server.json.tmpl` | The sensitive config `stackly-fake-server serve` reads: `greeting`, `instanceName`, and `authToken` |
| `versions.json` | The runtime artifacts, written by hand: one `fixture` artifact with the version of the `stackly-fake-server` crate |
| `tests/scenarios.yaml` | The scenarios the conformance harness runs: `default`, and `slow-ready` with `readyDelayMs: 1500` |
| `README.md` | This page; Stackly never reads it |

The secret `authToken` is never written in a file of the package: the daemon
generates it, keeps it in the Keychain, and hands it to `init` on stdin.

## Commands

Start a new package from this one, then check and test it:

```bash
cargo xtask provider new mytool
cargo xtask provider check providers/mytool
cargo xtask provider test providers/mytool
```

The report of a test run is `target/provider-test/<runId>/report.json`.

| Command | What it does |
| --- | --- |
| `cargo xtask provider new <name>` | Copies this package into `providers/<name>/` as `testing/<name>` |
| `cargo xtask provider check <dir>` | Reads and validates a package without running anything |
| `cargo xtask provider test <dir>` | Runs every scenario through a daemon in a temporary root and writes a report |
| `cargo xtask provider freeze <dir>` | Adds the package revision to the frozen compatibility corpus |
| `cargo xtask codegen` | Writes `versions.json` of an `official` package from `runtimes/inventory.toml` |

## From example to official package

1. Add the runtime to `runtimes/inventory.toml` with its spike evidence.
2. Change `metadata.id` to `official/<name>`, and set `software`, `category`,
   and `runtime.executable`.
3. Delete the hand written `versions.json` and run `cargo xtask codegen`.
4. Write `tests/scenarios.yaml` for the modes the provider supports.
5. Run `cargo xtask provider freeze` on the package.
6. Add one native test per runtime line in
   `apps/daemon/tests/provider_conformance.rs`.
7. Move the provider from `[[planned]]` to `[[provider]]` in
   `providers/catalog.toml`.
