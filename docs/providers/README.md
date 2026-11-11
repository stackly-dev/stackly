# Provider guide

This guide is for anyone who wants to add a tool to Stackly as a provider package. A provider package is a folder of YAML, JSON, and templates in [`providers/`](../../providers/). You write no Rust and no Swift: Stackly's daemon reads the package and runs the tool for you.

## Before you start

You need:

- A Mac with Apple Silicon on macOS 27.
- [rustup](https://rustup.rs). The toolchain pinned in `rust-toolchain.toml` installs itself on the first build.
- The Xcode Command Line Tools (`xcode-select --install`), for the C compiler some crates need.

No runtime download is needed for the quick start: the example package runs `stackly-fake-server`, a small test server built from this repository. Cargo may download crates on the first build if its cache is empty.

## Quick start

From the root of a fresh clone, these four commands check and test the example package, make your own package from it, and test that too:

<!-- docs-walkthrough -->
```bash
cargo xtask provider check providers/example
cargo xtask provider test providers/example
cargo xtask provider new mytool
cargo xtask provider test providers/mytool
```

Each test run writes a report to `target/provider-test/<runId>/report.json` and prints its path on the last line of stderr (`provider test report: <path>`). [testing.md](testing.md) explains what the report holds.

`cargo xtask provider new mytool` writes the package `testing/mytool` in `providers/mytool/`. A package in the `testing` namespace runs only in the test harness and never ships in the app catalog, so you can change it freely while you learn.

## Pages

| Page | What it covers |
| --- | --- |
| [authoring.md](authoring.md) | Writing a package step by step, and moving it to the official catalog |
| [manifest-reference.md](manifest-reference.md) | Every field of `provider.yaml` |
| [settings-reference.md](settings-reference.md) | Every keyword of `schemas/settings.schema.json` |
| [versions-reference.md](versions-reference.md) | Every field of `versions.json` |
| [testing.md](testing.md) | The validator, the conformance harness, its report, and native evidence |
| [compatibility.md](compatibility.md) | How the manifest format changes, deprecation, and migration |
| [review.md](review.md) | The checklist a maintainer follows before merging a provider |
| [trying-locally.md](trying-locally.md) | Importing your package into your own Stackly, approving it, and revoking or removing it |

## From idea to merged provider

1. **Scaffold.** Run `cargo xtask provider new <name>` and fill in the package ([authoring.md](authoring.md)).
2. **Validate the schema.** Run `cargo xtask provider check providers/<name>` until it prints `ok` ([testing.md](testing.md)).
3. **Test with fixtures locally.** Run `cargo xtask provider test providers/<name>` until the verdict is `pass`, and attach the report to your pull request.
4. **Native compatibility evidence.** A maintainer runs the real runtime on their Mac and records the evidence ([testing.md](testing.md#fixture-evidence-and-native-evidence)).
5. **Source, license, and security review.** A maintainer checks where the runtime comes from, its license, and what the package lets the runtime do ([review.md](review.md)).
6. **Maintainer review.** The maintainer works through the rest of [review.md](review.md) and asks for changes in the pull request.
7. **Merge.** The merged revision never changes again; any later change raises `metadata.revision`.

Open an issue with the provider proposal form before step 1 if you are unsure the tool fits Stackly. How to send a pull request in general is in [CONTRIBUTING.md](../../CONTRIBUTING.md).
