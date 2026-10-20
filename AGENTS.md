# Stackly

Stackly installs, runs, and watches native developer tools (PostgreSQL, MySQL, NATS, ClickHouse, Valkey, Traefik) on one Mac: many versions and many instances, no Docker, no cloud account. One Rust daemon per user (`stacklyd`) owns the registry and every process. The Rust CLI (`stackly`) and the SwiftUI app only talk to it over IPC. The feature 1 scaffold is in place: nine cargo workspace members, the `StacklyClient` and `StacklyUI` Swift packages, and the Xcode project, each holding one small real piece of content.

## Stack

- **Language / Runtime**: Rust 1.99.1, edition 2024, pinned in `rust-toolchain.toml` (core, daemon, CLI). Swift 6 language mode with full strict concurrency on Xcode 27 (macOS app). Target is macOS 27 on Apple Silicon only (`aarch64-apple-darwin`).
- **Framework**: tokio (multi thread runtime), SwiftUI, clap v4 derive.
- **Key dependencies**: serde_json over a private Unix socket (no TCP listener); typeshare and schemars, which generate Swift types and JSON Schema from `stackly-protocol`; rusqlite `bundled` with one writer thread; thiserror; tracing with JSON output; jiff (UTC, RFC 3339); uuid v7; serde_yaml_ng (never serde_yml).
- **Package manager**: cargo workspace with every third party version in `[workspace.dependencies]` (add a crate only when code uses it), local SwiftPM packages `StacklyClient` and `StacklyUI`, and `cargo xtask` as the task runner. Full decision: [spec 0001](docs/specs/0001-stack-arsitektur/index.md).

## Build approach

Journey (satu user journey dibangun lengkap, termasuk semua state, error, dan polish, sebelum journey berikutnya dimulai).

## Commands

From the repo root. All of these work on the feature 1 scaffold (spec 0001).

```bash
git config core.hooksPath .githooks   # once per clone: turns on the pre commit hook
cargo test -p <crate> [test_name_filter]
cargo test -p stackly-cli --test daemon_status   # the CLI package is stackly-cli; its binary is stackly
cargo test -p stacklyd --test operations   # operation engine over the socket; timing tests hold the spec bounds only under cargo xtask evidence run
STACKLY_ROOT=$(mktemp -d) cargo run -q -p stacklyd -- --print-roots   # resolved roots and socket path, creates nothing
STACKLY_LOG=debug cargo run -q -p stacklyd   # the daemon; STACKLY_LOG is a tracing filter, default info
cargo run -q -p stackly-cli -- daemon status --json
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo fmt --all
cargo deny check            # licenses, RustSec advisories, bans, sources
cargo xtask codegen          # rerun after any change in stackly-protocol
cargo xtask codegen --check  # fails when committed generated files drift
cargo xtask provider check [<dir>...]   # provider packages, every folder in providers/ by default: ok <id> <revision> <sha256>, or one line per problem
STACKLY_UPDATE_GOLDEN=1 cargo test -p stackly-core --test provider_packages   # rewrite the provider plan and render goldens
cargo xtask check-deps       # crate dependency direction and unsafe rules
cargo xtask fuzz list        # fuzz targets; also seed, build, and run <target> [--time <s>] (300 s default)
cargo xtask fuzz seed        # rewrite fuzz/seeds/ from protocols/fixtures/ after a fixture change
cargo xtask build-app        # xcodebuild, scheme Stackly
swift test --package-path clients/swift
cargo check --target aarch64-pc-windows-msvc -p stackly-domain -p stackly-platform -p stackly-protocol -p stackly-core
```

## Specs

Stored in `docs/specs/`. Format: `docs/specs/NNNN-slug/index.md`, with the reasoning in `rationale.md`. Specs replace the ADR folder the PRD proposes. What to build, in which order, and each feature's status live in `docs/scope/` (`index.md` holds the legend). The suggested flow per feature is `/architect` when it is tagged needs a decision, then `/develop`, `/check verify`, and `/test`; features tagged `· GA` add `/check review` and `/document`; `/sync` after a change lands. A feature's next step is its first unticked box.

`prd.md` is input for writing scope and specs only. Never take requirements, decisions, or acceptance criteria from it while building. If a scope or spec is missing something, update it through `/scope` or `/architect`, or ask. A spec beats an Agent Skill when they conflict, and scope and spec beat the mockup on behavior.

## Rules

- Clean Architecture mapped onto crates. `stackly-domain` (entities, invariants, state machines) is the center and imports no Stackly crate, tokio, or libc. `stackly-core` holds thin use cases that call ports, such as the OS traits in `stackly-platform`. `stackly-platform-macos` and `stackly-storage` are adapters. `stacklyd`, `stackly`, and the SwiftUI app are presentation. Dependencies point inward only, and `cargo xtask check-deps` enforces the full list in spec 0001 ("Aturan arah dependency").
- Domain, platform, protocol, and core never touch macOS APIs and must pass `cargo check` for `aarch64-pc-windows-msvc`.
- `stackly-ipc` (`crates/ipc`) is the socket transport for `stacklyd` and `stackly-cli`: among Stackly crates it depends only on `stackly-protocol` (dev: `stackly-testkit`, `stackly-evidence`) and holds no `unsafe`. No crate depends on the `stacklyd` library, and non test code in `crates/ipc`, `apps/daemon`, and `apps/cli` never names a TCP or UDP socket. `cargo xtask check-deps` enforces all three (spec 0006).
- `stacklyd` is a library plus a thin `main.rs`. In process tests start the server through `stacklyd::server::start` with a shortened `DaemonConfig`; binary tests use `stackly_testkit::SpawnedDaemon`.
- Every mutation runs as an operation of `stackly_core::operation::OperationEngine` (spec 0007). In `stacklyd`, `ipc::operation` maps the engine's views to the `operation` DTOs, and a method that starts an operation goes through `ipc::operation::start_operation` (key check, fingerprint, begin). The doc hidden `Handlers::test_starting_method` is a test only starting method until feature 10 adds the first contract one (`provisional: F10`).
- Only the daemon writes the registry, starts processes, and creates IDs and paths. The CLI never runs provider binaries, and the GUI never links Rust.
- A provider package is a folder in `providers/<name>/` with `provider.yaml` (`stackly/v1alpha1`), `schemas/settings.schema.json`, `templates/*.tmpl`, and `versions.json`; an optional `README.md`, `LICENSE-NOTICES.md`, and `tests/` are never read, and anything else is rejected (spec 0009). `cargo xtask codegen` writes each `versions.json` from `runtimes/inventory.toml` (only `providers/fake/versions.json` is hand written) and the editor JSON Schemas in `schemas/provider/` from the raw manifest types in `stackly-core`. The `fake-command` fixture package lives in `crates/core/tests/fixtures/packages/`, and `testing/` packages never enter the app catalog.
- Boundaries carry DTOs from `stackly-protocol`, never domain types; `stacklyd` does the mapping. The wire is camelCase JSON behind a 4 byte big endian length (1 MiB control frames, 64 KiB log batches, checked before allocating). Enums with data use `tag = "type"`, `content = "content"`. Generated files are never edited by hand, and the fixtures in `protocols/fixtures/` are the shared contract.
- Wire rules for every DTO in `stackly-protocol`: counts, sequences, and revisions use the `u64` newtypes in `wire.rs` (capped at `MAX_SAFE_INTEGER`), never a plain `u64`; an open enum ends in an `Unrecognized` variant that newer values fall back to (Swift `.unrecognized` through `OpenEnums.swift`); no `BTreeMap`, which typeshare cannot map; optional fields take `#[serde(default)]`; and every params type implements `Validate` (spec 0006).
- Any contract change bumps `ProtocolVersion::CURRENT` in `crates/protocol/src/version.rs` in the same commit, minor for an addition and major for a break. `cargo xtask codegen` records the digest in `protocols/contract-versions.json` and refuses a changed contract under a recorded version (`--amend` only while that version is not on main). A new method in the `methods!` table says `recovery allowed` or `recovery refused`.
- Errors use thiserror in libraries with stable categories in `stackly-protocol`, and CLI exit codes come from those categories. anyhow only in `xtask` and tests. The daemon and CLI check env vars, paths, and config once at startup and fail fast with a clear message.
- No `unwrap()`, Swift force unwrap, or `try!` outside tests; `expect("...")` only with a message that names the invariant. `unsafe` only in `stackly-platform-macos`, each block with a `// SAFETY:` comment.
- Every crate inherits the workspace lints with `[lints] workspace = true` and never writes its own `[lints]` table; `cargo xtask check-deps` rejects a crate that does not.
- Rust API style from the scaffold: `#[must_use]` on constructors and pure getters, a `# Errors` section on every public fallible function, `#[non_exhaustive]` on public error enums and on structs that will grow, and error messages in lowercase with no trailing period. Rust test names are snake_case sentences that state the behavior. Code a later feature replaces is marked `// provisional: F<n>`.
- Every public item in library crates and Swift packages has a doc comment. Code, comments, doc comments, test names, error messages, and commits are in English; scope, spec, and review docs stay in Indonesian. Commits follow `type(scope): subject`.
- Tests come after each build (`/test`). Domain gets plain unit tests with no fakes. Core gets unit tests against `stackly-testkit` fakes, not mocked adapters. Adapters, daemon, and supervisor get integration tests in each crate's `tests/` against real systems, each with its own ephemeral test root, ports, and guards that reap child processes; never the real Application Support folder, never global state. Take the root from `stackly_testkit::TestRoot` and ports from `stackly_testkit::allocate_port`, and use only fixture credentials, never real ones.
- Keep PRD IDs (`REQ`, `AC`, `NFR`, `OQ`, and the rest) stable in specs and tests. Test status starts at `NOT_RUN`, a fixture pass is not native runtime evidence, and missing hardware, binaries, or signing is recorded as `BLOCKED`.
- UI meets WCAG AA contrast, has VoiceOver labels, works fully from the keyboard, and follows Dynamic Type.

## Tooling

Installed by feature 2: `rustfmt.toml`, `clippy.toml`, `.swift-format`, `deny.toml`, and `.githooks/pre-commit`. Every check runs locally with the commands above.

- Format with rustfmt and swift-format (`.swift-format`). Clippy runs with default lints plus `clippy::unwrap_used`, and warnings fail (`-D warnings`). `missing_docs` is on for every crate through `[workspace.lints]`, so bins and each integration test file also start with a `//!` doc. `unwrap()` is allowed only inside `#[test]` functions and `#[cfg(test)]` modules, so integration test helpers use `expect`.
- swift-format ships with the Xcode 27 toolchain (`xcrun swift-format`), so it needs no separate install. It skips `Generated/`, which typeshare writes with tabs.
- Pre commit: a plain git hook, no Node or extra tools, running `cargo fmt --check`, clippy on the changed crates, and `swift-format lint`. Tests run by hand with the commands above.
- No CI for now: development focuses on this Mac, and every check runs by hand with the commands above. Native tests and evidence runs also happen on this Mac, as spec 0002 describes ("Menjalankan test di Mac ini").
- Builds never remap source paths.
- `fuzz/` is its own cargo workspace (root `exclude = ["fuzz"]`) on a pinned nightly, `nightly-2026-10-17` in `fuzz/rust-toolchain.toml` and `FUZZ_TOOLCHAIN` in `xtask/src/fuzz.rs`, with cargo-fuzz 0.13.2 (`CARGO_FUZZ_VERSION`). Install them with `rustup toolchain install nightly-2026-10-17 --profile minimal` and `cargo install --locked cargo-fuzz --version 0.13.2`. `fuzz/` sits outside `cargo fmt`, clippy, and the pre commit hook. When a crate the fuzz targets use gains a dependency, refresh `fuzz/Cargo.lock` with `cargo +nightly-2026-10-17 fetch --manifest-path fuzz/Cargo.toml`, or `cargo xtask fuzz build` refuses it.
- `typeshare-cli` is pinned in xtask. The pin is 1.13.4 (`TYPESHARE_CLI_VERSION` in `xtask/src/codegen.rs`); install it with `cargo install --locked typeshare-cli --version 1.13.4`.
- Markdown follows `.markdownlint.jsonc`: no line length limit, only `<a>` as inline HTML, compact tables.

## Git

- integration: on
- branch prefix: feat/
- commit: per-milestone
- before every commit: update the knowledge graph in `graphify-out/` so it matches what is being committed. Run `graphify update .`, which re-extracts code with no LLM. When the commit also changes docs, specs, or scope, run `/graphify . --update` instead, because `graphify update` skips non-code files. If `graphify-out/` is missing, build it once with `/graphify .`.

## Agent skills

- [rust-best-practices](.claude/skills/rust-best-practices/): `apollographql/skills`, idiomatic Rust, ownership, error handling, tests
- [rust-async-patterns](.claude/skills/rust-async-patterns/): `wshobson/agents`, tokio, async traits, concurrency

Declined: rust-engineer, github-actions (callstackincubator), github-actions-templates, rust-cli, rust-observability, serde-code-review, tokio-patterns, cargo-fuzz and tokio Unix socket Agent Skills and MCP servers (spec 0006)

MCP servers: GitHub MCP server (recommended), SQLite MCP pointed only at test roots (recommended)

## Context files

- [apps/macos/AGENTS.md](apps/macos/AGENTS.md): SwiftUI app and `StacklyUI`, Swift conventions and skills, mockup rules
- [clients/swift/AGENTS.md](clients/swift/AGENTS.md): `StacklyClient` package, generated contract types, fixture tests

_Drafted by /audit from the repo, worth a quick human pass. Edit freely: once a line stops matching this draft, later runs treat it as curated and will flag rather than overwrite it._
