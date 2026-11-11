# Testing a provider package

Two tools check a package. The validator reads it and runs nothing. The conformance harness runs it through a real daemon in a throwaway root. Run both before you open a pull request.

## The validator

```bash
cargo xtask provider check providers/mytool
```

`cargo xtask provider check` needs no daemon. With no folder it checks every folder in `providers/`. For each package it prints one line:

- `ok <id> <revision> <manifest_sha256>` when the package may be published;
- otherwise one line per error, `<folder>: <file> <location>: <code>: <message>`, and no `ok` line.

A warning prints the same line after `warning: `. The task exits 1 when a package is not publishable. Without a folder, it also treats a warning in a repository package as a failure, and reports `not frozen` for a catalog provider whose revision is missing from the compatibility corpus.

`stackly provider validate <dir>` asks a running daemon to run the same validator on a folder and prints the same lines, then a count such as `0 error(s), 1 warning(s)`. Start the daemon first, against a throwaway root (see [CONTRIBUTING.md](../../CONTRIBUTING.md#run-from-source-safely)). It exits 0 when the package is publishable and 2 (`cli.provider_invalid`) otherwise; `--json` prints the full result. Both tools give the same findings, because they share one validator.

### Findings

Every finding has a severity and an origin.

| Severity | Meaning |
| --- | --- |
| `error` | The package cannot be published until you fix it |
| `warning` | The package works, but uses something that is going away, such as a deprecated `apiVersion` (`deprecated_api_version`) |

| Origin | Where it comes from |
| --- | --- |
| `package` | Reading and validating the files: layout, YAML and JSON syntax, the manifest, the settings schema, the templates, `versions.json` |
| `authoring` | Rules for new packages that do not stop an older package from loading, such as a port token in an initialize step |
| `scenarios` | `tests/scenarios.yaml` |

Two words sum this up:

- **loadable**: no `error` finding has origin `package`, so Stackly can read the package.
- **publishable**: no finding is an `error`, so the package may be merged and shipped.

A `warning`, such as `deprecated_api_version`, never makes a package unloadable or unpublishable. Only `cargo xtask provider check` without a folder fails on it, so a package in this repository never ships with one.

## The conformance harness

```bash
cargo xtask provider test providers/mytool
```

`cargo xtask provider test <dir>` builds `stackly-fake-server`, starts its own daemon inside a temporary root, and runs every scenario of the package through the same IPC calls the app uses. It validates the package and installs the runtime once per run, while it prepares the root. Per scenario it finds the package in the catalog, checks that the runtime is installed, reads the create form, creates an instance, starts it, waits for `healthy`, reads the connection strings, checks the process, reads the logs, starts a second instance when `multiInstance` is on, reloads when `reload` is on, restarts, stops, checks that nothing is left, and starts the stopped instance again on the same data.

The process checks hold every provider to the same contract: the main process keeps the instance lock open on fd 3, does not re execute itself, stays in the foreground, and leaves no process, lock, or open port behind after a stop.

| Flag | What it does |
| --- | --- |
| `--line <line>` | Picks the runtime line when the package has more than one artifact |
| `--scenario <name>` | Runs only that scenario; give it more than once for several. A name the package does not have fails the run at the `validate` step with exit 1, before any root is made, and the message names the scenarios the package has: `unknown scenario <name>; the package has <names>` |
| `--report <file>` | Writes the report to that file instead of the default path |
| `--keep` | Keeps the data folder of the temporary root, for you to look at; the run prints its path on a `kept` line, with the temporary folder written as `$TMPDIR` |

The harness accepts packages in the `official` and `testing` namespaces only. It never downloads anything: a `testing` package runs on `stackly-fake-server`, and an official package needs its artifact in the runtime cache already, otherwise the verdict is `blocked`.

### Exit codes

| Code | Verdict |
| --- | --- |
| 0 | `pass`: every scenario passed and the root was cleaned up |
| 1 | `fail`: a scenario, the validation, or the cleanup failed |
| 2 | `blocked`: the run could not start, for example a runtime that is not in the cache |
| 130 | Interrupted: the first Ctrl+C stops the run in order, writes the report with `interrupted: true`, cleans up the root, and exits 130 |

### The report

The report is `target/provider-test/<runId>/report.json`, and its path is the last line of stderr: `provider test report: <path>`. Its `schema` is `stackly.provider-conformance/v1`. The fields you will read most:

| Field | What it holds |
| --- | --- |
| `verdict` | `pass`, `fail`, or `blocked` |
| `kind` | `fixture` for a fixture artifact, `native` for a real runtime |
| `package` | The folder, ID, revision, digest, and `apiVersion` that ran |
| `validation` | The validator findings |
| `scenarios` | Each scenario with its `outcome` and every step's `status`, duration, and `failure` with the step, code, and message |
| `handover` | `proven` or `unproven`, see [review.md](review.md#daemon-restart-handover) |
| `cleanup` | Whether the root was removed and which processes were left |
| `blocked` | The reason and the owner when the verdict is `blocked` |

Paths in the report write your home folder as `~` and the temporary folder as `$TMPDIR`, and no step detail ever holds a settings value, so you can attach the report to a pull request as it is.

### Test roots and cleanup

The harness always works in its own temporary root with its own daemon. It never touches your real Stackly folder in Application Support, your Keychain (secrets go to an in memory store), or a daemon you have running. It marks each root it makes and removes only a root it marked itself, owned by you, with no process still using it.

When a run is cut short (a second Ctrl+C, a crash), its root stays behind. Remove every such root of a run that is no longer alive with:

```bash
cargo xtask provider test --cleanup
```

The same command also removes the data folders that `--keep` left, since their runs have ended.

### Scenarios that exercise failures

`providers/fake` (`testing/fake`) has settings that make `stackly-fake-server` misbehave, so a scenario can test how the harness and the daemon react: `readyDelayMs` delays readiness, `crashAfterMs` and `crashExitCode` crash the server after a while, `logLinesPerSecond` and `logLineBytes` flood the log, and `failInit` and `initDelayMs` act on the init step. A scenario that should fail says so with `expect: start_fails` or `expect: create_fails` ([authoring.md](authoring.md#scenarios)); a failure the scenario did not expect fails the run, and so does a success it did not expect.

## Fixture evidence and native evidence

A harness run on a `testing` package is fixture evidence: it proves the package format, the lifecycle, and the process contract on `stackly-fake-server`. It is not evidence that a real runtime works. Native evidence runs the real runtime on a maintainer's Mac and is written to `evidence/` as a record.

- A pull request from a contributor brings the harness report. That is enough to start the review.
- A maintainer runs the native evidence for an official package: the runtime spike, the native conformance test per line in `apps/daemon/tests/provider_conformance.rs`, and, only when the provider should survive a planned daemon restart, the native launchd test. The commands are in [review.md](review.md#native-evidence).
