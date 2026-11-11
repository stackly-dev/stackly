# Contributing to Stackly

Thanks for helping. This guide covers what you need to send a change, from a clean clone to a pull request.

## Ways to contribute

- **Report a bug** with the bug report form.
- **Propose a provider**: a new tool for Stackly to install and run.
- **Write a provider package**. A simple provider is a folder of YAML, JSON, and templates in `providers/`; it needs no new Rust or Swift code.
- **Improve the docs**.
- **Change the core**: the daemon, the CLI, the app, or the IPC protocol. A change to the core, or to how Stackly runs processes, gets a closer review and usually needs a spec first; open an issue before you start.

## Before you start

- The project has adopted the Contributor Covenant, version 2.1, as its Code of Conduct.
- Ask a question or start a discussion with one of the issue forms.
- Never report a vulnerability in an issue. Use the private channel in [SECURITY.md](SECURITY.md).

## Set up

You need:

- A Mac with Apple Silicon on macOS 27.
- [rustup](https://rustup.rs). The pinned toolchain in `rust-toolchain.toml` installs itself on the first build.
- Xcode 27, only for the macOS app.
- `typeshare-cli` 1.13.4, only when you change the IPC contract.

If you only work on a provider package, the Rust toolchain is enough.

Turn on the pre commit hook once per clone (rustfmt, clippy, and swift-format on what you change):

```bash
git config core.hooksPath .githooks
```

## Build and check locally

This repository has no CI. Please run the checks yourself and paste the output into your pull request.

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test -p <crate>          # each crate you changed
cargo deny check               # licenses, RustSec advisories, bans, sources
cargo xtask check-deps         # crate dependency direction and the unsafe policy
cargo xtask codegen --check    # generated files match the contract
cargo xtask provider check     # every provider package in providers/
cargo xtask docs check         # links, commands, and the community files
```

When you add or change a provider package, also run the conformance harness on it and attach its report:

```bash
cargo xtask provider test providers/<name>
```

When you change `providers/catalog.toml` or a package listed in it, also run:

```bash
cargo xtask catalog check
```

When you change Swift code, also run:

```bash
swift test --package-path clients/swift
swift test --package-path apps/macos/Packages/StacklyUI
cargo xtask build-app
```

## Run from source safely

Always run a development build against a throwaway root, never against your real Stackly folder in Application Support. `STACKLY_ROOT` picks the root for both the daemon and the CLI, and `STACKLY_LOG` sets the daemon's log filter (default `info`).

```bash
export STACKLY_ROOT=$(mktemp -d)
STACKLY_LOG=debug cargo run -q -p stacklyd
```

Then, in a second terminal with the same `STACKLY_ROOT`:

```bash
cargo run -q -p stackly-cli -- daemon status --json
```

## Contributing a provider

The [provider guide](docs/providers/README.md) takes you from a clean clone to a pull request. In short:

1. Start from the example with `cargo xtask provider new <name>` ([authoring.md](docs/providers/authoring.md)).
2. Fill in `provider.yaml`, the settings schema, the templates, and `tests/scenarios.yaml`, using the reference pages.
3. Run `cargo xtask provider check providers/<name>` until it prints `ok`.
4. Run `cargo xtask provider test providers/<name>` until the verdict is `pass` ([testing.md](docs/providers/testing.md)).
5. Open a pull request with the harness report and fill in the provider checklist of the template; the maintainer follows [review.md](docs/providers/review.md).

## Pull requests

- One change per pull request, on a branch from `main`.
- Commit messages follow `type(scope): subject`, in English.
- Fill in the pull request template, including the output of the local checks.
- A maintainer runs the checks again on their own machine before merging.
- A pull request from a fork never gets signing or release credentials.

## Where decisions live

Specs in `docs/specs/` and the scope in `docs/scope/` are written in Indonesian. If you want an English summary of a decision, ask in an issue. Code, comments, error messages, and the public docs are in English.

The full set of project rules is in [AGENTS.md](AGENTS.md); this guide points there instead of copying it.

## Licensing

Stackly is licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT), at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in Stackly by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.

- Only send work you have the right to send.
- Material copied from upstream, such as an example config, must be listed in the package's `LICENSE-NOTICES.md` with its license and source.
- A contribution made with the help of an AI tool is still the responsibility of the person who sends it.

Stackly's license does not cover the third party software it installs and runs. The license of each runtime, per version, and whether its binary is downloaded from upstream or built by maintainers, are recorded in `runtimes/inventory.toml`. A provider whose runtime has a license outside the `allow` list of `deny.toml` can still be proposed (that list is for linked dependencies only), but it needs the release owner's license approval before it enters the catalog.
