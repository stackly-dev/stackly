# Writing a provider package

This page walks you through a provider package from the scaffold to a pull request. Each step points at the reference page that lists every field: [manifest-reference.md](manifest-reference.md), [settings-reference.md](settings-reference.md), and [versions-reference.md](versions-reference.md).

## Package layout

A package is one folder in `providers/`, named after the name part of `metadata.id`:

```text
providers/<name>/
├── provider.yaml                 # the manifest, at most 256 KiB
├── versions.json                 # the runtime artifacts, at most 1 MiB
├── schemas/
│   └── settings.schema.json      # the settings form, the only file in schemas/
├── templates/
│   └── *.tmpl                    # config templates, at most 32, no subfolders
├── tests/
│   └── scenarios.yaml            # the scenarios the harness runs
├── README.md                     # optional, never read
└── LICENSE-NOTICES.md            # required for an official package, never read by the loader
```

The loader rejects anything else in the folder, every symlink, and every file that is not UTF-8. Every template must be used by an entry of `configFiles`. `tests/scenarios.yaml` is the only file in `tests/` that anything reads.

## Start from the example

```bash
cargo xtask provider new mytool
```

This copies [`providers/example/`](../../providers/example/) into `providers/mytool/` as the package `testing/mytool`. It changes only the `id`, the `displayName`, and the title of the package README, then checks the copy and prints `ok` and the next commands. Use `--out <dir>` to write the package somewhere else. The name follows the rules of a provider name (lowercase letters, digits, and single `-`, starting with a letter), and `example` is taken. The command never writes over a folder that already exists.

The new package is valid at once: `cargo xtask provider check providers/mytool` and `cargo xtask provider test providers/mytool` both pass before you change anything. It runs `stackly-fake-server`, so you can learn the format without downloading a runtime.

A new folder in `providers/` also needs two repository changes in the same pull request, for a `testing` package too, not only when it moves to the official catalog: add the folder name to `PROVIDER_WORDS` in `xtask/src/check_deps/source_scan.rs` (until you do, `cargo test -p xtask` fails with `<name> is not listed`), and write its create form fixture with `STACKLY_UPDATE_GOLDEN=1 cargo test -p stacklyd --test create_form_fixtures`, then list the new `protocols/fixtures/instance/create-form-<name>.json` in `protocols/fixtures/instance/index.json` with the type `CreateForm`.

## Fill in the package

Work in this order, and run `cargo xtask provider check providers/mytool` after each change.

1. **`provider.yaml`.** Set `metadata` (`displayName`, `category`, `description`), the endpoints in `resources.endpoints`, the config files, the init steps, how to start and stop the process, and the readiness probe. The main process must stay in the foreground and keep fd 3 open; the harness checks both. See [manifest-reference.md](manifest-reference.md).
2. **`schemas/settings.schema.json`.** Describe each setting the user may change, with its limits and when a change takes effect (`apply`). A password or token is a field with `secret: true`: Stackly keeps its value in the Keychain and never writes it in the package. See [settings-reference.md](settings-reference.md).
3. **`templates/*.tmpl`.** Write each config file with `{{ settings.<field> }}` and the other tokens. The `format` of the config file decides how each value is escaped. Templates have no expressions: only values, `#each`, `#if`, and comments.
4. **`tests/scenarios.yaml`.** List the scenarios the harness should run, as below.
5. **`versions.json`.** A `testing` package keeps its hand written `fixture` artifact. An official package gets this file from `cargo xtask codegen` instead (see [Moving to the official catalog](#moving-to-the-official-catalog)).

## Scenarios

`tests/scenarios.yaml` tells the harness which settings to try. From `providers/example/tests/scenarios.yaml`:

```yaml
schema: stackly.provider-scenarios/v1
scenarios:
  - name: default
  - name: slow-ready
    settings: { readyDelayMs: 1500 }
```

| Field | Rules |
| --- | --- |
| `schema` | Exactly `stackly.provider-scenarios/v1` |
| `scenarios` | 1 to 16 scenarios |
| `name` | A valid instance name of at most 24 characters, unique in the file |
| `settings` | Values for the settings schema, on top of its defaults; never a secret field, because the daemon generates every secret |
| `expect` | `ready` (default), `start_fails`, or `create_fails` |

Without the file the harness runs one scenario, `default`, with the schema defaults. A scenario that sets a field the schema does not have, or a value outside its limits, is a `scenarios` finding of `cargo xtask provider check`.

## Moving to the official catalog

A `testing` package never ships. To make it an official provider, the package and the repository change together in one pull request, in this order:

1. **Pin the runtime.** Add an `[[artifact]]` entry for each runtime version to `runtimes/inventory.toml`, with the real download URL and digest. `cargo xtask runtime pin --url <https>` downloads the file and prints the values to copy. Never write a URL or a digest you have not checked. The spike evidence for each artifact follows the runtime flow (`cargo xtask runtime spike`), and a maintainer runs it.
2. **Change the identity.** Set `metadata.id` to `official/<name>`, `metadata.software` to the `provider` of the inventory entries, the `category`, and `runtime.executable`. The folder name must equal the name part of the new ID.
3. **Generate `versions.json`.** Delete the hand written file and run `cargo xtask codegen`, which writes it from the inventory. `cargo xtask codegen --check` must stay clean.
4. **Check the scan list.** The folder name is already in `PROVIDER_WORDS` in `xtask/src/check_deps/source_scan.rs` ([Start from the example](#start-from-the-example)). Add the name part of the new ID too when it differs. Until every folder and catalog ID is listed, the `xtask` test `the_provider_words_cover_every_provider_folder_and_catalog_id` fails.
5. **Update the create form fixture.** Run `STACKLY_UPDATE_GOLDEN=1 cargo test -p stacklyd --test create_form_fixtures` again, which rewrites the form fixture of every folder in `providers/` for the new identity and runtime, and commit the changed file.
6. **Add native tests.** Add one native test per runtime line to `apps/daemon/tests/provider_conformance.rs`, like the existing NATS and PostgreSQL tests. A maintainer runs them ([testing.md](testing.md#fixture-evidence-and-native-evidence)).
7. **List it in the catalog.** Add a `[[provider]]` entry to `providers/catalog.toml` (or move it from `[[planned]]`), and run `cargo xtask catalog check`.
8. **Freeze the revision.** Run `cargo xtask provider freeze providers/<name>`, which copies this revision into the compatibility corpus, and commit the new entry. Freeze refuses a package that is not a `[[provider]]` of the catalog, so it comes after step 7, and `cargo xtask provider check` without arguments reports `not frozen` for a catalog provider until you run it. See [compatibility.md](compatibility.md).
9. **Add `LICENSE-NOTICES.md`.** Three headings, in order: `## Package` (the package files are `MIT OR Apache-2.0`), `## Runtime` (the upstream license of each line, a link to it, and where the binary comes from), and `## Third party material in this package` (files copied from upstream, or `None.`). Copy the shape of [`providers/nats/LICENSE-NOTICES.md`](../../providers/nats/LICENSE-NOTICES.md). `cargo xtask docs check` compares it with the inventory.

After the merge, every change to a file that counts toward the package digest (`provider.yaml`, `schemas/settings.schema.json`, and the templates) raises `metadata.revision` and runs `cargo xtask provider freeze` again in the same commit. A merged revision never changes.

A provider whose instances should keep running across a planned daemon restart needs one more step; see [review.md](review.md#daemon-restart-handover).

## Next

- [testing.md](testing.md): what `provider check` and `provider test` report, and how to read the report.
- [review.md](review.md): what the maintainer checks before merging.
