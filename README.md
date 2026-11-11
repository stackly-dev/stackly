# Stackly

![Stackly macOS app mockup](docs/mockups/stackly-overview.png)

Stackly installs, runs, and watches native developer tools (PostgreSQL, MySQL, NATS, ClickHouse, Valkey, Traefik) on one Mac: many versions and many instances, no Docker, no cloud account.

One Rust daemon per user (`stacklyd`) owns the registry and every process. The command line client (`stackly`) and the SwiftUI app only talk to it over a private Unix socket.

**Status:** early scaffold. The workspace builds and both binaries print their version, but nothing is installed or run yet.

## Requirements

- A Mac with Apple Silicon on macOS 27
- Xcode 27 (Swift 6.4), selected with `xcode-select`
- [rustup](https://rustup.rs). The pinned Rust toolchain in `rust-toolchain.toml` installs itself on first build.
- `typeshare-cli` 1.13.4, only when you change the IPC contract:

```bash
cargo install --locked typeshare-cli --version 1.13.4
```

## Common commands

Turn on the pre commit hook once per clone (rustfmt, clippy, and swift-format on what you change):

```bash
git config core.hooksPath .githooks
```

```bash
cargo build --workspace             # daemon, CLI, and libraries
cargo test --workspace              # Rust tests
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo deny check                    # licenses, RustSec advisories, bans, sources
cargo xtask codegen                 # regenerate Swift types and JSON Schemas from stackly-protocol
cargo xtask codegen --check         # fail when committed generated files drift
cargo xtask check-deps              # crate dependency direction and the unsafe policy
cargo xtask build-app               # build the macOS app with xcodebuild
swift test --package-path clients/swift
```

## Layout

| Path | What lives there |
| --- | --- |
| `crates/` | Rust libraries: domain, platform ports, protocol, core use cases, the macOS adapter, test kit |
| `apps/daemon` | `stacklyd`, the per user daemon |
| `apps/cli` | `stackly`, the command line client |
| `apps/macos` | The SwiftUI app and the `StacklyUI` package |
| `clients/swift` | `StacklyClient`, the Swift side of the IPC contract |
| `protocols/` | Generated JSON Schemas and the JSON fixtures shared by Rust and Swift tests |
| `xtask/` | Repo tasks behind `cargo xtask` |
| `docs/` | Scope, specs, and the UI mockup |

The architecture and its rules are in [spec 0001](docs/specs/0001-stack-arsitektur/index.md).

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT), at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in Stackly by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.

This license covers what the project writes in this repository. It does not cover the third party software Stackly installs and runs; the license and origin of each runtime are recorded in `runtimes/inventory.toml`.
