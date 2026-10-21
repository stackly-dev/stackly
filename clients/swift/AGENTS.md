# StacklyClient (Swift package)

## Overview

The Swift side of the IPC contract: DTOs generated from `stackly-protocol`, plus (from feature 13) the socket transport the app uses to talk to `stacklyd`. Swift conventions and skills live in [apps/macos/AGENTS.md](../../apps/macos/AGENTS.md); this file adds only what differs here.

## Key files

| File | Owns |
|---|---|
| `Sources/StacklyClient/Generated/StacklyProtocol.swift` | Contract types written by `cargo xtask codegen`, committed |
| `Sources/StacklyClient/Generated/OpenEnums.swift` | `init(from:)` for every enum with `case unrecognized`, mapping a value from a newer daemon to `.unrecognized`; written by codegen |
| `Sources/StacklyClient/Generated/ContractInfo.swift` | The contract digest and `ProtocolVersion` the handshake sends; written by codegen |
| `Sources/StacklyClient/Generated/ContractNames.swift` | Event kind and method names for the transport (spec 0013); written by codegen |
| `Sources/StacklyClient/Endpoint/`, `Transport/`, `Wire/`, `Connection/` | Socket path and endpoint checks, framing over `DispatchIO`, three stage frame decode, and `DaemonConnection` with handshake, calls, and subscribe (spec 0013) |
| `Sources/StacklyClient/Link/`, `Sync/` | `DaemonLink` with reconnect and heartbeat, and `SyncEngine` with snapshot plus cursor (spec 0013) |
| `Sources/StacklyTestSupport/` | Product for test targets only: the `.evidence` trait and records (spec 0002) and `FixtureCredential` (spec 0011) |
| `Tests/StacklyClientTests/Support/` | `FakeDaemonServer`, `SpawnedDaemon`, `ManualClock`, and `RepoPaths` for transport and integration tests |
| `Sources/StacklyClient/StacklyJSON.swift` | `StacklyJSON.decoder()` and `encoder()`: millisecond UTC timestamps (`yyyy-MM-dd'T'HH:mm:ss.SSS'Z'`) for every contract read and write |
| `../../protocols/fixtures/` | Shared JSON fixtures, one file per case, read by both Rust and Swift tests |
| `../../typeshare.toml` | Generator settings, including the `Sendable` and `Equatable` decorators |

## Commands

```bash
swift test --package-path clients/swift                  # from the repo root
swift test --package-path clients/swift --filter <name>  # one test
cargo build -p stacklyd                                  # before the integration tests; they use target/debug/stacklyd or STACKLY_DAEMON_BIN (an absolute path), else end as BLOCKED
cargo xtask codegen                                      # regenerate after a protocol change
```

## Conventions

- No `MainActor` default isolation in this package: socket IO runs off the main actor. Generated DTOs are `Sendable`, so they can cross to the UI.
- `StacklyClient` has no third party dependency, opens only a Unix socket, and never spawns a process or uses a network API (`URLSession`, `NWConnection`, `AF_INET`); `SourceRuleTests` enforces it (spec 0013).
- Never edit `Generated/` by hand. Change the Rust type in `stackly-protocol`, then rerun codegen; `cargo xtask codegen --check` fails on drift.
- Read and write contract JSON only through `StacklyJSON`; a plain `JSONDecoder` or `JSONEncoder` gets the wire timestamps wrong.
- An enum with `case unrecognized` is open: handle `.unrecognized` wherever it shows, apart from any real `unknown` state. `ErrorCategory` is closed and rejects a value it does not know.
- Fixture tests compare decoded values, not raw JSON text. Swift encodes UUIDs in uppercase while Rust writes lowercase, and every reader treats them as case insensitive.
- A test that needs a secret value takes it from `FixtureCredential` in `StacklyTestSupport`, which prints as `FixtureCredential(****)`; never a literal credential (spec 0011).

## Gotchas

- Tests find fixtures through a path relative to `#filePath`, so builds must never remap source paths (`-file-prefix-map`).
- Enums with data use the adjacently tagged shape (`type` and `content`), the shape typeshare supports for Swift. Enums without data are plain strings.

## Related specs

- [0001 Stack dan arsitektur](../../docs/specs/0001-stack-arsitektur/index.md)
- [0006 Protokol IPC dan daemon](../../docs/specs/0006-protokol-ipc-daemon/index.md)
- [0013 App shell SwiftUI dan Swift client](../../docs/specs/0013-app-shell-swift-client/index.md)

_Drafted by /audit from the repo, worth a quick human pass. Edit freely: once a line stops matching this draft, later runs treat it as curated and will flag rather than overwrite it._
