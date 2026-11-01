# Review, feat/f20-form-from-pinned-package (feature 20, milestone 7), 2026-11-01

**Reviewed by**: sonnet, fresh context (author on opus)
**Scope**: 35 files (about 20 reviewed by hand, generated Swift and schemas checked only for drift), branch commits ba32c497, 46e7b647, bf86134b vs 9865261e
**Verdict**: Approve with nits

## Summary

`instance.get_create_form` gains an optional `revision`. With it, the daemon judges the runtime, then the provider name, then resolves the package through a `resolve_package` now shared with `gather`, then compares the software. The CLI sends `--provider` and `--revision` to the form and drops the fallback that swallowed `catalog.unavailable` and `instance.provider_not_found`. The change matches spec 0020 AC-1, AC-18 and milestone 7 steps 21 and 22. I found no blocker or major. `cargo xtask codegen --check`, `cargo xtask check-deps`, and clippy with `-D warnings` on `stacklyd`, `stackly-cli` and `stackly-core` are clean. I did not re-run the test suites.

## Minor

### 🟡 Catalog gate runs before the runtime lookup, so the AC-1 order does not hold while the catalog loads, `apps/daemon/src/ipc/instance/form.rs:38`
**Problem**: `catalog_state(core)?` runs at the top of the handler, before the runtime is read. While the catalog loads (`core.catalog()` is `None`) every form request, with or without `revision`, answers `daemon.starting` (subsystem `catalog`), even for an unknown runtime or a failed runtime. AC-1 says that with a revision the order is runtime first, then provider, and the new test `with_a_revision_the_runtime_is_judged_before_the_provider` pins that only for a loaded daemon.
**Why it matters**: The behavior is defensible and not a regression. The loading state is `daemon.starting`, not `catalog.unavailable` (the "catalog is loading" text is the subsystem health row), and the CLI retries `daemon.starting` for up to 10 s (`apps/cli/src/session.rs:35`), so `stackly instance create --revision` right after daemon start works. It is correct that the pinned path waits, because a `Loaded` catalog can change the outcome (matching revision, `Planned` entry). The gap is that the spec and the test do not say so. `instance.create` has the same gate, so form and create agree.
**Suggested fix**: Add one sentence to AC-1 (or the milestone 7 note) saying the catalog gate comes first and a loading catalog answers `daemon.starting`. Add a test, or a note in the test header, for the loading window. Do not move the gate unless the spec wants the pinned form served while the catalog loads.

### 🟡 Parity with create is asserted on codes only, not on details, `apps/daemon/tests/supervisor/create_form_pinned.rs:35`
**Problem**: `assert_form_refuses_like_create` compares `error.code()` of the form with the first preflight error code. The spec also says the error detail carries `provider` and `revision` (and `providerSoftware` for the mismatch). Only `providerSoftware` and `reason` are asserted. The new `revision` detail on `provider_not_found` and `provider_ambiguous` (`form.rs:form_error`) is not.
**Why it matters**: Swift and CLI clients show these details. A silent regression in `form_error` would pass every test.
**Suggested fix**: In the helper, also compare `detail("provider")` and `detail("revision")` between the form error and the preflight error row.

### 🟡 Several branches of the new path have no test, `apps/daemon/tests/supervisor/create_form_pinned.rs`
**Problem**: Not covered: catalog `Unavailable` (as opposed to `NotConfigured`) with a pinned revision; an `Invalid` entry with a pinned revision (the spec text names it next to `Planned`); `suggested_name` avoiding a taken name for a pinned package; the daemon-side `ProviderUnavailable` arm; `--dry-run` with `--revision` in the CLI (the new `&CreateForm` plumbing); a Swift test with a non-nil `revision` (`CreateClientTests.swift` only passes `nil`).
**Why it matters**: These are branching and error paths. The two outcomes most likely to differ from create are `Invalid` plus a pin, and the taken-name suggestion.
**Suggested fix**: Add one `Invalid` or `Unavailable` case and one taken-name case to `create_form_pinned.rs`. Add a CLI `--dry-run --revision` assertion in `create_real.rs`. Add a Swift test that sends a `revision`.

## Nits

- ⚪ `apps/daemon/src/ipc/instance/form.rs:84`, `pinned_form` takes `Runtime` by value, so the caller clones it and later uses the original for the summary. Take `&Runtime`.
- ⚪ `apps/daemon/src/ipc/instance/form.rs:135`, `select_provider(state, &runtime, None)` re-checks the runtime state that was just checked a few lines above. Harmless but redundant.
- ⚪ `apps/daemon/src/ipc/instance/form.rs:173`, the `ProviderProblem::Unavailable` arm cannot occur with a revision (it only arises when `revision` is `None`). The spec lists three problems; the fourth arm is fine defensively, but a short comment would say why it exists.
- ⚪ `apps/daemon/src/ipc/instance/form.rs:160`, the join-error mapping is a copy of the one at line 58, and drops the `JoinError`. Log it as is done for the read error.
- ⚪ `clients/swift/Tests/StacklyClientTests/Contract/ContractTests.swift:137`, the test is still named `protocolIsOnePointSixteen` but now asserts 1.17. The Rust counterpart was renamed.
- ⚪ `protocols/fixtures/ipc/request-instance-get-create-form.json`, the existing fixture now carries `"revision": null`, which the roundtrip needs. The "1.16 client without the field" case is then only proven by the `decode` fixtures. That matches the spec, but note it in the fixture index if you want the intent visible.

## Strengths

- One `resolve_package` serves `gather` and the form handler. `find_pinned` was narrowed to the ports it needs (registry, platform, layout) rather than `CreateDeps`, and no second copy exists. `check-deps` and the Windows-checked crates stay clean.
- The form stays read only: no re-pin, no writes. The test snapshots the provider folder before and after and checks that no instance exists.
- The AC-1 order is implemented and tested with the sharp case (a failed runtime with an unpinned revision answers `supervisor.runtime_unavailable`, not `provider_not_found`). Form-versus-preflight parity is checked per case.
- The contract bump is complete: `ProtocolVersion` 1.17, `contract-versions.json`, digest, Rust and Swift version tests, new accept and reject fixtures, `Validate` with the same length bound as `provider.revision`.
- The CLI change removes a fallback instead of adding a branch: `form()` returns `CreateForm`, `--json` and exit 6 for `catalog.unavailable` are unchanged and tested, and a real-daemon test proves typed `--set` values (JSON boolean and integer) with no catalog.

## Test coverage

Good for the main paths: real daemon, no catalog and loaded catalog, planned entry plus pin, two digest folders, mismatched software, runtime order, no writes, and a CLI test against a scripted daemon and one against `stacklyd`. The gaps are listed in the third Minor finding and are all on secondary branches. Tests use `Site`, `TestRoot` and fixture data only. Names are sentences and carry `// covers: AC-1` or an `AC-18` doc line.
