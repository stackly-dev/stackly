## What and why

What this pull request changes and why. Link the issue it closes, for example `Closes #123`.

## Kind of change

- [ ] Provider package
- [ ] Core or daemon
- [ ] CLI
- [ ] macOS app
- [ ] Docs

## Checks I ran locally

This repository has no CI, so please run the checks yourself and paste the output below. See [CONTRIBUTING.md](https://github.com/stackly-dev/stackly/blob/main/CONTRIBUTING.md#build-and-check-locally).

- [ ] `cargo fmt --all --check`
- [ ] `cargo clippy --workspace --all-targets --locked -- -D warnings`
- [ ] `cargo test -p <crate>` for each crate I changed
- [ ] `cargo deny check`
- [ ] `cargo xtask check-deps`
- [ ] `cargo xtask codegen --check`
- [ ] `cargo xtask provider check`
- [ ] `cargo xtask docs check`
- [ ] Swift changes only: `swift test` for the package I changed and `cargo xtask build-app`

```text
Paste the output here.
```

## Provider checklist

Only for a pull request that adds or changes a provider package.

- [ ] `cargo xtask provider check` is clean for the package
- [ ] The harness `report.json` from `cargo xtask provider test` is attached
- [ ] `metadata.revision` is raised for every change to a file in the package digest (`README.md` and `LICENSE-NOTICES.md` are not in it)
- [ ] `cargo xtask catalog check` is clean, if the package is in `providers/catalog.toml`
- [ ] Each `runtimes/inventory.toml` entry uses the real upstream URL and digest
- [ ] `LICENSE-NOTICES.md` is up to date
- [ ] No Rust core or Swift code is changed

## Licensing

- [ ] I have the right to submit this and agree to license it under MIT OR Apache-2.0
