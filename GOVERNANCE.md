# Governance

This document says who looks after Stackly, how decisions are made, and who answers which kind of issue.

## Roles

| Role | Responsibilities | Current holder |
| --- | --- | --- |
| Core maintainer | Domain, core, daemon, CLI, and the IPC protocol; has the final say | @ojiepermana |
| Platform maintainer (macOS) | The macOS adapter, the SwiftUI app, and signing | @ojiepermana |
| Provider owner | One provider package: its manifest, versions, compatibility evidence, and issues | @ojiepermana for every `official/` provider |
| Security contact | Receives private reports and runs the advisory process in [SECURITY.md](SECURITY.md) | @ojiepermana |
| Release owner | Releases, the runtime license gate, and artifact uploads | @ojiepermana |

Today one person holds every role. There is no `CODEOWNERS` file yet. When a second person takes a role, that pull request adds `.github/CODEOWNERS` and updates this table.

## How decisions are made

- The core maintainer decides.
- A decision that carries weight (a data model, a protocol change, a new execution primitive, a policy like this one) is written down as a spec in [`docs/specs/`](docs/specs/).
- Anyone can propose a change by opening an issue.
- Disagreements are worked out in that issue, and the maintainer writes down the reason for the outcome there.

## Official and reviewed

- `official` means the package lives in `providers/` in this repository under the `official/` namespace, has a provider owner, and ships in the bundled catalog.
- `reviewed` means that revision of the package passed the maintainer's provider review when it was merged.

Neither word means a package is free of risk forever, and a review covers one revision only. `reviewed` is a word for this process, not a label in the app.

## Provider owners

| Provider | Owner | Status |
| --- | --- | --- |
| `official/nats` | @ojiepermana | In the bundled catalog |
| `official/postgresql` | @ojiepermana | In the bundled catalog |
| `official/traefik` | @ojiepermana | In the bundled catalog |

A provider owner:

- answers the issues about their provider;
- keeps its versions and compatibility evidence current;
- marks an upstream incompatibility as such, with a link to the upstream issue.

To become a provider owner, get a provider pull request accepted and say that you are willing to look after it.

When an owner stops responding, the provider is marked as having no owner. After that it may be pulled from the bundled catalog. Pulling a provider never deletes user data.

## Issue triage

New issues get the `needs-triage` label. A maintainer replaces it with one of the four category labels.

| Category | Label | Who answers | Usual outcome |
| --- | --- | --- | --- |
| Core bug | `core` | Core or platform maintainer | A fix in this repository |
| Provider bug | `provider` | Provider owner | A new package revision |
| Upstream incompatibility | `upstream` | Provider owner | The version is marked, linked to the upstream issue, and not fixed in Stackly |
| Configuration question | `configuration` | Anyone; a maintainer closes it | An answer with a docs link; the docs are fixed when the question comes back |

Vulnerabilities never go into issues; see [SECURITY.md](SECURITY.md).

## Compatibility and deprecation

The provider manifest format, `stackly/v1alpha1`, is defined in [spec 0009](docs/specs/0009-provider-engine-manifest-schema/index.md). A page for provider authors on schema compatibility and deprecation is not written yet; until it is, that spec is the reference.

The IPC protocol between the daemon, the CLI, and the app has its own version and contract digest ([spec 0006](docs/specs/0006-protokol-ipc-daemon/index.md)).

## Review without CI

This repository has no CI. Maintainers run the checks on their own machine before a merge. Catalog publishing and signing happen only from a trusted release context, never from a script in a pull request.

## Changing this document

Change it through a pull request that the core maintainer approves.
