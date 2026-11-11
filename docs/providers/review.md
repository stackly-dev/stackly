# Provider review checklist

A maintainer works through this list before merging a pull request that adds or changes a provider package. Contributors can read it to know what will be asked. A merged revision passed this review once; the review covers that revision only.

## Read the diff first

Running the tests, the harness, or `cargo xtask docs walkthrough` on the branch of a pull request runs the code of that pull request on your Mac: build scripts, tests, and the runtime the package starts. Read the whole diff before you run anything, and stop at anything outside the package folder that you did not expect.

## Validator

- [ ] `cargo xtask provider check providers/<name>` prints `ok` with no warning.
- [ ] `cargo xtask provider check` without arguments is clean for the whole repository.
- [ ] `cargo xtask codegen --check` is clean, so `versions.json` matches `runtimes/inventory.toml`.
- [ ] `cargo xtask catalog check` is clean when `providers/catalog.toml` or a package listed in it changed.
- [ ] `cargo xtask docs check` is clean, including the package's `LICENSE-NOTICES.md`.
- [ ] `cargo test -p xtask` passes: a new folder in `providers/` needs its name in `PROVIDER_WORDS` (`xtask/src/check_deps/source_scan.rs`).

## Harness report

- [ ] The pull request carries the `report.json` of `cargo xtask provider test` for the revision in the pull request: same `package.revision` and `package.manifestSha256` as `cargo xtask provider check` prints.
- [ ] Its `verdict` is `pass`, and `cleanup` shows the root removed and no process left.
- [ ] Run the harness again yourself on the same commit, after reading the diff, and compare the verdicts.
- [ ] `tests/scenarios.yaml` covers the modes the provider offers, such as each setting that changes how the server starts.

## Native evidence

A maintainer runs these on their own Mac, with the artifacts in the runtime cache. Each writes a record under `evidence/`.

```bash
cargo xtask evidence run --kind native -- cargo test -p xtask --test native_runtime -- --ignored
cargo xtask evidence run --kind native -- cargo test -p stacklyd --test provider_conformance -- --ignored --test-threads 1
```

- [ ] The runtime spike passes for every new artifact (the first command). `cargo xtask runtime record <report.json>` then copies its records into `evidence/runtime/<runId>/` (the two commit flow of spec 0003: the inventory change first, then the evidence on that clean tree), and only after that `cargo xtask codegen` writes the compatibility record into `versions.json`.
- [ ] The native conformance test passes for every runtime line of the package (the second command; one test per line in `apps/daemon/tests/provider_conformance.rs`).
- [ ] The launchd test passes for every line, only when the provider should survive a planned daemon restart (see [Daemon restart handover](#daemon-restart-handover)).

A contributor's pull request does not need to bring native evidence; a missing artifact or a step that needs a GUI login is recorded as `BLOCKED` with a reason and an owner, never as a pass.

## Compatibility corpus

- [ ] A new official revision has its entry in `crates/core/tests/fixtures/compat/`, written by `cargo xtask provider freeze` in the same commit that raises `metadata.revision` or adds the provider to the catalog.
- [ ] No existing entry changed. See [compatibility.md](compatibility.md).

## Source and license

- [ ] Every artifact has an entry in `runtimes/inventory.toml` with the real upstream URL and SHA-256, checked by a maintainer. No URL or digest is made up, and none points at a personal mirror.
- [ ] `LICENSE-NOTICES.md` names the upstream license of each line and where the binary comes from, matching the inventory.
- [ ] Every file copied or adapted from upstream, such as an example config, is listed under `## Third party material in this package` with its license and source.
- [ ] A runtime license outside the `allow` list of `deny.toml` has the release owner's approval before the provider enters the catalog.

## Security

- [ ] Every command runs an executable of the runtime itself (`${runtime.executable}` or `${runtime.executables.<alias>}`), never a program from `PATH` or a shell script.
- [ ] No secret reaches a process through argv: secrets go through `env`, stdin, or a sensitive config file in `${instance.secrets}`.
- [ ] Every endpoint binds to loopback by default.
- [ ] `openWith` and `connection` hold no password, and `remedies` only describe steps; Stackly never runs them.
- [ ] Settings that reach `args` cannot become new flags (the validator's `argument_injection_risk` rule), and a free text setting is limited by a `pattern` or an `enum` where the tool would read it as syntax.
- [ ] The package uses only the instance folders it lists in `permissions.managedFilesystem`.

## Revision

- [ ] A change to a file that counts toward the package digest (`provider.yaml`, `schemas/settings.schema.json`, the templates) raises `metadata.revision`.
- [ ] A revision that is already on `main` never changes after the merge.

## Daemon restart handover

Passing the harness never lets a provider's instances keep running across a planned daemon restart. A provider joins `HANDOVER_PROVEN` (`crates/core/src/supervisor/handover.rs`) only after a native launchd test for each line passes, with the ID of its evidence record:

```bash
cargo xtask evidence run --kind native -- cargo test -p stacklyd --test native_launchd -- --ignored --test-threads 1
```

- [ ] A new entry in `HANDOVER_PROVEN` names a passing native launchd record. A manifest can never add itself to the list.
- [ ] Without an entry, the harness report shows `handover: unproven`, and the provider's instances are stopped and started again around a planned restart. That is fine for a provider in the catalog.
