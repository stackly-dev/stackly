# StacklyClient (Swift package)

## Overview

The Swift side of the IPC contract: DTOs generated from `stackly-protocol`, plus (from feature 13) the socket transport the app uses to talk to `stacklyd`. Swift conventions and skills live in [apps/macos/AGENTS.md](../../apps/macos/AGENTS.md); this file adds only what differs here.

## Key files

| File | Owns |
|---|---|
| `Sources/StacklyClient/Generated/StacklyProtocol.swift` | Contract types written by `cargo xtask codegen`, committed |
| `../../protocols/fixtures/` | Shared JSON fixtures, one file per case, read by both Rust and Swift tests |
| `../../typeshare.toml` | Generator settings, including the `Sendable` and `Equatable` decorators |

## Commands

```bash
swift test --package-path clients/swift                  # from the repo root
swift test --package-path clients/swift --filter <name>  # one test
cargo xtask codegen                                      # regenerate after a protocol change
```

## Conventions

- No `MainActor` default isolation in this package: socket IO runs off the main actor. Generated DTOs are `Sendable`, so they can cross to the UI.
- Never edit `Generated/` by hand. Change the Rust type in `stackly-protocol`, then rerun codegen; `cargo xtask codegen --check` fails on drift.
- Fixture tests compare decoded values, not raw JSON text. Swift encodes UUIDs in uppercase while Rust writes lowercase, and every reader treats them as case insensitive.

## Gotchas

- Tests find fixtures through a path relative to `#filePath`, so builds must never remap source paths (`-file-prefix-map`).
- Enums with data use the adjacently tagged shape (`type` and `content`), the shape typeshare supports for Swift. Enums without data are plain strings.

## Related specs

- [0001 Stack dan arsitektur](../../docs/specs/0001-stack-arsitektur/index.md)

_Drafted by /audit from the repo, worth a quick human pass. Edit freely: once a line stops matching this draft, later runs treat it as curated and will flag rather than overwrite it._
